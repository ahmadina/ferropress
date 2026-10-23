//! `lab inventory`: the interoperability facts of the reference platform, recorded under
//! `contracts/inventory/` — declared functions, classes, hooks, constants and globals, and the REST routes,
//! options and database tables of a fresh install.
//!
//! The lab's own script (`lab/app/inventory.php`) produces the facts; this module validates them, puts every
//! list in a canonical order and writes one JSON Lines file per section: a header line, then one item per
//! line, so a diff names exactly the items that changed.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::Context as _;
#[cfg(test)]
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Where the inventory is written, relative to the repository root.
pub(crate) const INVENTORY_DIR: &str = "contracts/inventory";
/// Inside the app container: the inventory script copied in by `lab/app/Dockerfile`.
pub(crate) const SCRIPT: &str = "/opt/ferropress/inventory.php";

const SCHEMA: u32 = 1;

/// One parameter of a function or method signature.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Param {
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) type_name: Option<String>,
    /// Default value expression, in compact canonical form.
    pub(crate) default: Option<String>,
    pub(crate) by_ref: bool,
    pub(crate) variadic: bool,
    /// Declared with a visibility modifier in a constructor, which also declares a property.
    pub(crate) promoted: bool,
}

/// One declaration of a function; conditional and stub declarations each keep their own signature.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FunctionDeclaration {
    /// `path:line` relative to the platform root.
    pub(crate) location: String,
    pub(crate) params: Vec<Param>,
    pub(crate) return_type: Option<String>,
    pub(crate) by_ref: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Function {
    pub(crate) name: String,
    pub(crate) declarations: Vec<FunctionDeclaration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ClassKind {
    Class,
    Interface,
    Trait,
    Enum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Visibility {
    Public,
    Protected,
    Private,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Property {
    pub(crate) name: String,
    pub(crate) visibility: Visibility,
    #[serde(rename = "static")]
    pub(crate) is_static: bool,
    pub(crate) readonly: bool,
    #[serde(rename = "type")]
    pub(crate) type_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is an independent PHP modifier recorded as is in the inventory format"
)]
pub(crate) struct Method {
    pub(crate) name: String,
    pub(crate) visibility: Visibility,
    #[serde(rename = "static")]
    pub(crate) is_static: bool,
    #[serde(rename = "abstract")]
    pub(crate) is_abstract: bool,
    #[serde(rename = "final")]
    pub(crate) is_final: bool,
    pub(crate) params: Vec<Param>,
    pub(crate) return_type: Option<String>,
    pub(crate) by_ref: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Class {
    /// Fully qualified name without a leading backslash.
    pub(crate) name: String,
    pub(crate) kind: ClassKind,
    #[serde(rename = "abstract")]
    pub(crate) is_abstract: bool,
    #[serde(rename = "final")]
    pub(crate) is_final: bool,
    /// Parent class, or parent interfaces for an interface, in declaration order.
    pub(crate) extends: Vec<String>,
    /// Implemented interfaces, in declaration order.
    pub(crate) implements: Vec<String>,
    /// Used traits, in declaration order.
    pub(crate) traits: Vec<String>,
    pub(crate) constants: Vec<String>,
    pub(crate) properties: Vec<Property>,
    pub(crate) methods: Vec<Method>,
    pub(crate) declarations: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum HookKind {
    Action,
    Filter,
}

/// One place a hook is fired.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HookSite {
    pub(crate) location: String,
    /// Arguments passed to callbacks, or `None` when they come from a computed array.
    pub(crate) args: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Hook {
    pub(crate) kind: HookKind,
    /// The name with every computed part written as `{expression}`.
    pub(crate) name: String,
    pub(crate) dynamic: bool,
    /// Fired through the deprecated-hook functions at least once.
    pub(crate) deprecated: bool,
    pub(crate) sites: Vec<HookSite>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Constant {
    pub(crate) name: String,
    pub(crate) declarations: Vec<String>,
    /// Defined once a front-end request has finished booting.
    pub(crate) defined_at_boot: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Global {
    pub(crate) name: String,
    /// Files that import the variable with a `global` statement.
    pub(crate) files: Vec<String>,
    /// Set once a front-end request has finished booting.
    pub(crate) set_at_boot: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RestRoute {
    /// Route pattern as registered.
    pub(crate) route: String,
    pub(crate) namespace: String,
    pub(crate) methods: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OptionName {
    pub(crate) name: String,
    pub(crate) autoload: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum TableScope {
    /// One per network, created only by multisite installs.
    Multisite,
    /// Shared by every site.
    Global,
    /// One per site.
    Site,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Column {
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) type_name: String,
    pub(crate) nullable: bool,
    pub(crate) default: Option<String>,
    pub(crate) extra: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Index {
    pub(crate) name: String,
    pub(crate) unique: bool,
    /// Columns in index order, with a prefix length as `column(length)`.
    pub(crate) columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Table {
    /// Name without the table prefix.
    pub(crate) name: String,
    pub(crate) scope: TableScope,
    /// Whether a single-site install creates it; columns and indexes are listed only then.
    pub(crate) created: bool,
    /// Columns in table order.
    pub(crate) columns: Vec<Column>,
    /// Indexes in the order the database reports them.
    pub(crate) indexes: Vec<Index>,
}

/// The complete inventory.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Inventory {
    pub(crate) schema: u32,
    pub(crate) platform_version: String,
    pub(crate) functions: Vec<Function>,
    pub(crate) classes: Vec<Class>,
    pub(crate) hooks: Vec<Hook>,
    pub(crate) constants: Vec<Constant>,
    pub(crate) globals: Vec<Global>,
    pub(crate) rest_routes: Vec<RestRoute>,
    pub(crate) options: Vec<OptionName>,
    pub(crate) tables: Vec<Table>,
}

/// First line of every section file.
#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: u32,
    platform_version: String,
    section: String,
}

/// Sort key for `path:line` locations: by path, then by line number.
fn location_key(location: &str) -> (&str, u64) {
    match location.rsplit_once(':') {
        Some((path, line)) => (path, line.parse().unwrap_or(u64::MAX)),
        None => (location, 0),
    }
}

fn sort_locations(locations: &mut [String]) {
    locations.sort_by(|a, b| location_key(a).cmp(&location_key(b)));
}

/// PHP function, method and class names are case-insensitive; order by folded name, then exact name.
fn folded(name: &str) -> (String, &str) {
    (name.to_ascii_lowercase(), name)
}

impl Inventory {
    /// Puts every list whose order carries no meaning into canonical order.
    pub(crate) fn canonicalize(&mut self) {
        self.functions
            .sort_by(|a, b| folded(&a.name).cmp(&folded(&b.name)));
        for function in &mut self.functions {
            function
                .declarations
                .sort_by(|a, b| location_key(&a.location).cmp(&location_key(&b.location)));
        }
        self.classes
            .sort_by(|a, b| folded(&a.name).cmp(&folded(&b.name)));
        for class in &mut self.classes {
            class.constants.sort();
            class.properties.sort_by(|a, b| a.name.cmp(&b.name));
            class
                .methods
                .sort_by(|a, b| folded(&a.name).cmp(&folded(&b.name)));
            sort_locations(&mut class.declarations);
        }
        self.hooks
            .sort_by(|a, b| (&a.name, a.kind).cmp(&(&b.name, b.kind)));
        for hook in &mut self.hooks {
            hook.sites
                .sort_by(|a, b| location_key(&a.location).cmp(&location_key(&b.location)));
        }
        self.constants.sort_by(|a, b| a.name.cmp(&b.name));
        for constant in &mut self.constants {
            sort_locations(&mut constant.declarations);
        }
        self.globals.sort_by(|a, b| a.name.cmp(&b.name));
        for global in &mut self.globals {
            global.files.sort();
        }
        self.rest_routes.sort_by(|a, b| a.route.cmp(&b.route));
        for route in &mut self.rest_routes {
            route.methods.sort();
        }
        self.options.sort_by(|a, b| a.name.cmp(&b.name));
        self.tables.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// Problems that make the inventory unusable: an unsupported schema or duplicate entries.
    pub(crate) fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.schema != SCHEMA {
            problems.push(format!(
                "inventory schema {} is not supported (expected {SCHEMA})",
                self.schema
            ));
        }
        let mut check = |section: &str, keys: Vec<String>| {
            let mut seen = BTreeSet::new();
            for key in keys {
                if !seen.insert(key.clone()) {
                    problems.push(format!("{section}: `{key}` is listed more than once"));
                }
            }
        };
        check(
            "functions",
            self.functions
                .iter()
                .map(|f| f.name.to_ascii_lowercase())
                .collect(),
        );
        check(
            "classes",
            self.classes
                .iter()
                .map(|c| c.name.to_ascii_lowercase())
                .collect(),
        );
        check(
            "hooks",
            self.hooks
                .iter()
                .map(|h| format!("{:?} {}", h.kind, h.name))
                .collect(),
        );
        check(
            "constants",
            self.constants.iter().map(|c| c.name.clone()).collect(),
        );
        check(
            "globals",
            self.globals.iter().map(|g| g.name.clone()).collect(),
        );
        check(
            "rest_routes",
            self.rest_routes.iter().map(|r| r.route.clone()).collect(),
        );
        check(
            "options",
            self.options.iter().map(|o| o.name.clone()).collect(),
        );
        check(
            "tables",
            self.tables.iter().map(|t| t.name.clone()).collect(),
        );
        problems
    }

    /// One-line counts per section.
    pub(crate) fn summary(&self) -> String {
        format!(
            "{} functions, {} classes, {} hooks, {} constants, {} globals, {} REST routes, {} options, \
             {} tables",
            self.functions.len(),
            self.classes.len(),
            self.hooks.len(),
            self.constants.len(),
            self.globals.len(),
            self.rest_routes.len(),
            self.options.len(),
            self.tables.len()
        )
    }

    /// The section files: name and JSON Lines content.
    pub(crate) fn render(&self) -> anyhow::Result<Vec<(&'static str, String)>> {
        Ok(vec![
            ("functions", self.section("functions", &self.functions)?),
            ("classes", self.section("classes", &self.classes)?),
            ("hooks", self.section("hooks", &self.hooks)?),
            ("constants", self.section("constants", &self.constants)?),
            ("globals", self.section("globals", &self.globals)?),
            (
                "rest-routes",
                self.section("rest-routes", &self.rest_routes)?,
            ),
            ("options", self.section("options", &self.options)?),
            ("tables", self.section("tables", &self.tables)?),
        ])
    }

    fn section<T: Serialize>(&self, name: &str, items: &[T]) -> anyhow::Result<String> {
        let header = Header {
            schema: self.schema,
            platform_version: self.platform_version.clone(),
            section: name.to_owned(),
        };
        let mut out = serde_json::to_string(&header)?;
        out.push('\n');
        for item in items {
            let line = serde_json::to_string(item)
                .with_context(|| format!("serializing a {name} entry"))?;
            writeln!(out, "{line}").context("formatting")?;
        }
        Ok(out)
    }

    /// Reads the section files written by [`Inventory::render`] from `dir`.
    #[cfg(test)]
    pub(crate) fn load(dir: &Path) -> anyhow::Result<Self> {
        let mut version = None;
        let mut read = |section: &str| -> anyhow::Result<String> {
            let path = dir.join(format!("{section}.jsonl"));
            let text =
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            let first = text.lines().next().unwrap_or_default();
            let header: Header = serde_json::from_str(first)
                .with_context(|| format!("parsing the header of {}", path.display()))?;
            anyhow::ensure!(
                header.section == section,
                "{} declares section `{}`",
                path.display(),
                header.section
            );
            anyhow::ensure!(
                header.schema == SCHEMA,
                "{} has schema {} (expected {SCHEMA})",
                path.display(),
                header.schema
            );
            match &version {
                None => version = Some(header.platform_version),
                Some(expected) => anyhow::ensure!(
                    *expected == header.platform_version,
                    "{} is for platform {} but other sections are for {expected}",
                    path.display(),
                    header.platform_version
                ),
            }
            Ok(text)
        };
        let functions = items(&read("functions")?, "functions")?;
        let classes = items(&read("classes")?, "classes")?;
        let hooks = items(&read("hooks")?, "hooks")?;
        let constants = items(&read("constants")?, "constants")?;
        let globals = items(&read("globals")?, "globals")?;
        let rest_routes = items(&read("rest-routes")?, "rest-routes")?;
        let options = items(&read("options")?, "options")?;
        let tables = items(&read("tables")?, "tables")?;
        Ok(Self {
            schema: SCHEMA,
            platform_version: version.unwrap_or_default(),
            functions,
            classes,
            hooks,
            constants,
            globals,
            rest_routes,
            options,
            tables,
        })
    }
}

#[cfg(test)]
fn items<T: DeserializeOwned>(text: &str, section: &str) -> anyhow::Result<Vec<T>> {
    text.lines()
        .enumerate()
        .skip(1)
        .map(|(index, line)| {
            serde_json::from_str(line)
                .with_context(|| format!("{section}.jsonl line {}", index + 1))
        })
        .collect()
}

/// Parses, validates and canonicalizes the inventory script's output.
pub(crate) fn parse(output: &str) -> anyhow::Result<Inventory> {
    let mut inventory: Inventory =
        serde_json::from_str(output).context("parsing the inventory script's output")?;
    inventory.canonicalize();
    let problems = inventory.problems();
    anyhow::ensure!(
        problems.is_empty(),
        "the inventory is invalid:\n  - {}",
        problems.join("\n  - ")
    );
    Ok(inventory)
}

/// Writes every section file into `dir`, replacing earlier ones.
pub(crate) fn write(inventory: &Inventory, dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    for (section, text) in inventory.render()? {
        let path = dir.join(format!("{section}.jsonl"));
        fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT: &str = r#"{
        "schema": 1,
        "platform_version": "7.1.3",
        "functions": [
            {"name": "zeta", "declarations": [
                {"location": "b.php:20", "params": [], "return_type": null, "by_ref": false},
                {"location": "b.php:3", "params": [{"name": "x", "type": "int", "default": "1", "by_ref": true, "variadic": false, "promoted": false}], "return_type": "void", "by_ref": false}
            ]},
            {"name": "Alpha", "declarations": [{"location": "a.php:1", "params": [], "return_type": null, "by_ref": true}]}
        ],
        "classes": [
            {"name": "Ns\\Thing", "kind": "class", "abstract": true, "final": false, "extends": ["Base"], "implements": ["B", "A"], "traits": [],
             "constants": ["Z", "A"],
             "properties": [{"name": "y", "visibility": "private", "static": true, "readonly": false, "type": null},
                            {"name": "x", "visibility": "public", "static": false, "readonly": true, "type": "int"}],
             "methods": [{"name": "run", "visibility": "protected", "static": false, "abstract": true, "final": false, "params": [], "return_type": null, "by_ref": false},
                         {"name": "Build", "visibility": "public", "static": true, "abstract": false, "final": true, "params": [], "return_type": "self", "by_ref": false}],
             "declarations": ["c.php:9"]},
            {"name": "Contract", "kind": "interface", "abstract": false, "final": false, "extends": [], "implements": [], "traits": [], "constants": [], "properties": [], "methods": [], "declarations": ["i.php:1"]}
        ],
        "hooks": [
            {"kind": "filter", "name": "init", "dynamic": false, "deprecated": false, "sites": [{"location": "x.php:100", "args": 1}, {"location": "x.php:9", "args": null}]},
            {"kind": "action", "name": "init", "dynamic": false, "deprecated": true, "sites": [{"location": "x.php:1", "args": 0}]}
        ],
        "constants": [{"name": "B", "declarations": ["k.php:10", "k.php:2"], "defined_at_boot": true}, {"name": "A", "declarations": [], "defined_at_boot": false}],
        "globals": [{"name": "post", "files": ["z.php", "a.php"], "set_at_boot": false}],
        "rest_routes": [{"route": "/b", "namespace": "n", "methods": ["POST", "GET"]}, {"route": "/a", "namespace": "", "methods": []}],
        "options": [{"name": "siteurl", "autoload": "on"}, {"name": "home", "autoload": "on"}],
        "tables": [
            {"name": "posts", "scope": "site", "created": true,
             "columns": [{"name": "ID", "type": "bigint unsigned", "nullable": false, "default": null, "extra": "auto_increment"}, {"name": "a", "type": "text", "nullable": true, "default": "x", "extra": ""}],
             "indexes": [{"name": "PRIMARY", "unique": true, "columns": ["ID"]}, {"name": "a", "unique": false, "columns": ["a(191)"]}]},
            {"name": "blogs", "scope": "multisite", "created": false, "columns": [], "indexes": []}
        ]
    }"#;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("xtask-inventory-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn parse_orders_every_unordered_list() {
        let inventory = parse(OUTPUT).unwrap();
        assert_eq!(
            inventory
                .functions
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            ["Alpha", "zeta"]
        );
        assert_eq!(
            inventory.functions[1]
                .declarations
                .iter()
                .map(|d| d.location.as_str())
                .collect::<Vec<_>>(),
            ["b.php:3", "b.php:20"]
        );
        assert_eq!(
            inventory.functions[1].declarations[0].params[0]
                .type_name
                .as_deref(),
            Some("int")
        );
        let class = &inventory.classes[1];
        assert_eq!(inventory.classes[0].name, "Contract");
        assert_eq!(class.implements, ["B", "A"], "declaration order is kept");
        assert_eq!(class.constants, ["A", "Z"]);
        assert_eq!(
            class
                .properties
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["x", "y"]
        );
        assert_eq!(
            class
                .methods
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            ["Build", "run"]
        );
        assert_eq!(
            inventory.hooks.iter().map(|h| h.kind).collect::<Vec<_>>(),
            [HookKind::Action, HookKind::Filter]
        );
        assert_eq!(inventory.hooks[1].sites[0].location, "x.php:9");
        assert_eq!(inventory.constants[1].declarations, ["k.php:2", "k.php:10"]);
        assert_eq!(inventory.globals[0].files, ["a.php", "z.php"]);
        assert_eq!(inventory.rest_routes[0].route, "/a");
        assert_eq!(inventory.rest_routes[1].methods, ["GET", "POST"]);
        assert_eq!(inventory.options[0].name, "home");
        assert_eq!(inventory.tables[0].name, "blogs");
        assert_eq!(
            inventory.tables[1].columns[0].name, "ID",
            "column order is kept"
        );
        assert_eq!(
            inventory.summary(),
            "2 functions, 2 classes, 2 hooks, 2 constants, 1 globals, 2 REST routes, 2 options, 2 tables"
        );
    }

    #[test]
    fn parse_rejects_duplicates_bad_schemas_and_unknown_fields() {
        let duplicate = OUTPUT.replace(r#""name": "zeta""#, r#""name": "ALPHA""#);
        let error = format!("{:#}", parse(&duplicate).unwrap_err());
        assert!(
            error.contains("functions: `alpha` is listed more than once"),
            "{error}"
        );

        let schema = OUTPUT.replacen(r#""schema": 1"#, r#""schema": 2"#, 1);
        let error = format!("{:#}", parse(&schema).unwrap_err());
        assert!(
            error.contains("inventory schema 2 is not supported"),
            "{error}"
        );

        let unknown = OUTPUT.replacen(r#""globals""#, r#""extra": [], "globals""#, 1);
        let error = format!("{:#}", parse(&unknown).unwrap_err());
        assert!(
            error.starts_with("parsing the inventory script's output"),
            "{error}"
        );
    }

    #[test]
    fn every_section_reports_duplicates() {
        let mut inventory = parse(OUTPUT).unwrap();
        inventory.classes.push(inventory.classes[0].clone());
        inventory.hooks.push(inventory.hooks[0].clone());
        inventory.constants.push(inventory.constants[0].clone());
        inventory.globals.push(inventory.globals[0].clone());
        inventory.rest_routes.push(inventory.rest_routes[0].clone());
        inventory.options.push(inventory.options[0].clone());
        inventory.tables.push(inventory.tables[0].clone());
        let sections: Vec<String> = inventory
            .problems()
            .iter()
            .map(|p| p.split(':').next().unwrap().to_owned())
            .collect();
        assert_eq!(
            sections,
            [
                "classes",
                "hooks",
                "constants",
                "globals",
                "rest_routes",
                "options",
                "tables"
            ]
        );
    }

    #[test]
    fn written_sections_load_back_identically() {
        let inventory = parse(OUTPUT).unwrap();
        let dir = temp_dir("roundtrip");
        write(&inventory, &dir).unwrap();
        let hooks = fs::read_to_string(dir.join("hooks.jsonl")).unwrap();
        let mut lines = hooks.lines();
        assert_eq!(
            lines.next(),
            Some(r#"{"schema":1,"platform_version":"7.1.3","section":"hooks"}"#)
        );
        assert!(
            lines
                .next()
                .unwrap()
                .starts_with(r#"{"kind":"action","name":"init""#)
        );
        assert_eq!(Inventory::load(&dir).unwrap(), inventory);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn load_rejects_missing_or_inconsistent_sections() {
        let inventory = parse(OUTPUT).unwrap();
        let dir = temp_dir("load");
        write(&inventory, &dir).unwrap();
        let path = dir.join("options.jsonl");
        let original = fs::read_to_string(&path).unwrap();

        let cases = [
            (
                original.replace(r#""section":"options""#, r#""section":"tables""#),
                "declares section `tables`",
            ),
            (
                original.replace(r#""schema":1"#, r#""schema":9"#),
                "has schema 9",
            ),
            (original.replace("7.1.3", "7.0.0"), "is for platform 7.0.0"),
            (
                original.replace(r#""autoload":"on""#, r#""autoload":1"#),
                "options.jsonl line 2",
            ),
            ("not json".to_owned(), "parsing the header of"),
        ];
        for (text, expected) in cases {
            fs::write(&path, text).unwrap();
            let error = format!("{:#}", Inventory::load(&dir).unwrap_err());
            assert!(error.contains(expected), "{expected}: {error}");
        }
        fs::remove_file(&path).unwrap();
        let error = format!("{:#}", Inventory::load(&dir).unwrap_err());
        assert!(error.starts_with("reading "), "{error}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn write_reports_unwritable_directories() {
        let file = temp_dir("blocked");
        fs::write(&file, b"not a directory").unwrap();
        let error = format!("{:#}", write(&parse(OUTPUT).unwrap(), &file).unwrap_err());
        assert!(error.starts_with("creating "), "{error}");
        fs::remove_file(file).unwrap();
    }

    #[test]
    fn locations_sort_by_path_then_line_number() {
        let mut locations = vec![
            "b.php:2".to_owned(),
            "a.php:10".to_owned(),
            "a.php:9".to_owned(),
            "a.php".to_owned(),
            "a.php:x".to_owned(),
        ];
        sort_locations(&mut locations);
        assert_eq!(
            locations,
            ["a.php", "a.php:9", "a.php:10", "a.php:x", "b.php:2"]
        );
    }

    #[test]
    fn committed_inventory_is_valid_and_canonical() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(INVENTORY_DIR);
        let inventory = Inventory::load(&dir).unwrap();
        assert!(
            inventory.problems().is_empty(),
            "{:?}",
            inventory.problems()
        );
        let mut canonical = inventory.clone();
        canonical.canonicalize();
        assert_eq!(
            canonical, inventory,
            "run `cargo xtask lab inventory` to regenerate"
        );
        for (section, text) in inventory.render().unwrap() {
            let committed = fs::read_to_string(dir.join(format!("{section}.jsonl"))).unwrap();
            assert!(
                committed == text,
                "{section}.jsonl is not in canonical form"
            );
        }
    }
}
