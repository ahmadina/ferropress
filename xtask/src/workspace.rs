//! Discovery of workspace members and their internal dependencies via `cargo metadata`.

use std::collections::HashSet;
use std::fmt;
use std::path::{Component, Path};

use anyhow::Context as _;
use cargo_metadata::{Metadata, MetadataCommand};

/// Top-level directory a workspace crate lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CrateLocation {
    /// A library crate under `crates/`.
    Crates,
    /// A binary crate under `apps/`.
    Apps,
    /// The `xtask` automation crate.
    Xtask,
    /// Anywhere else; not a sanctioned location.
    Other,
}

impl CrateLocation {
    /// Derives the location from a crate directory relative to the workspace root.
    pub(crate) fn from_relative_dir(dir: &Path) -> Self {
        let mut components = dir.components();
        let first = match components.next() {
            Some(Component::Normal(first)) => first.to_str(),
            _ => None,
        };
        let depth = 1 + components.count();
        match (first, depth) {
            (Some("crates"), 2) => Self::Crates,
            (Some("apps"), 2) => Self::Apps,
            (Some("xtask"), 1) => Self::Xtask,
            _ => Self::Other,
        }
    }
}

/// How one workspace crate depends on another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DependencyKind {
    /// `[dependencies]`
    Normal,
    /// `[dev-dependencies]`
    Development,
    /// `[build-dependencies]`
    Build,
}

impl DependencyKind {
    fn from_metadata(kind: cargo_metadata::DependencyKind) -> Self {
        match kind {
            cargo_metadata::DependencyKind::Development => Self::Development,
            cargo_metadata::DependencyKind::Build => Self::Build,
            // Unknown kinds are judged by the strictest rules.
            _ => Self::Normal,
        }
    }
}

impl fmt::Display for DependencyKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Normal => "normal",
            Self::Development => "dev",
            Self::Build => "build",
        })
    }
}

/// A dependency edge to another workspace member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Dependency {
    pub(crate) name: String,
    pub(crate) kind: DependencyKind,
}

/// A workspace member with its internal dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Member {
    pub(crate) name: String,
    pub(crate) location: CrateLocation,
    pub(crate) dependencies: Vec<Dependency>,
}

/// Loads all workspace members, sorted by name, using the workspace containing the current directory.
///
/// # Errors
///
/// Returns an error if `cargo metadata` cannot be run or its output cannot be parsed.
pub(crate) fn load() -> anyhow::Result<Vec<Member>> {
    let metadata = MetadataCommand::new()
        .no_deps()
        .exec()
        .context("failed to run `cargo metadata`")?;
    Ok(members(&metadata))
}

fn members(metadata: &Metadata) -> Vec<Member> {
    let packages = metadata.workspace_packages();
    let names: HashSet<&str> = packages
        .iter()
        .map(|package| package.name.as_str())
        .collect();

    let mut members: Vec<Member> = packages
        .iter()
        .map(|package| {
            let manifest_dir = package
                .manifest_path
                .parent()
                .unwrap_or(&package.manifest_path);
            let relative_dir = manifest_dir
                .strip_prefix(&metadata.workspace_root)
                .unwrap_or(manifest_dir);
            let dependencies = package
                .dependencies
                .iter()
                .filter(|dependency| {
                    dependency.path.is_some() && names.contains(dependency.name.as_str())
                })
                .map(|dependency| Dependency {
                    name: dependency.name.clone(),
                    kind: DependencyKind::from_metadata(dependency.kind),
                })
                .collect();
            Member {
                name: package.name.to_string(),
                location: CrateLocation::from_relative_dir(relative_dir.as_std_path()),
                dependencies,
            }
        })
        .collect();
    members.sort_by(|a, b| a.name.cmp(&b.name));
    members
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_recognizes_sanctioned_directories() {
        assert_eq!(
            CrateLocation::from_relative_dir(Path::new("crates/cms-hooks")),
            CrateLocation::Crates
        );
        assert_eq!(
            CrateLocation::from_relative_dir(Path::new("apps/server")),
            CrateLocation::Apps
        );
        assert_eq!(
            CrateLocation::from_relative_dir(Path::new("xtask")),
            CrateLocation::Xtask
        );
    }

    #[test]
    fn location_rejects_nested_or_unknown_directories() {
        for dir in [
            "crates",
            "crates/a/b",
            "apps",
            "xtask/sub",
            "tools/x",
            "",
            "../x",
        ] {
            assert_eq!(
                CrateLocation::from_relative_dir(Path::new(dir)),
                CrateLocation::Other,
                "{dir}"
            );
        }
    }

    #[test]
    fn dependency_kind_maps_cargo_kinds() {
        use cargo_metadata::DependencyKind as Cargo;
        assert_eq!(
            DependencyKind::from_metadata(Cargo::Normal),
            DependencyKind::Normal
        );
        assert_eq!(
            DependencyKind::from_metadata(Cargo::Development),
            DependencyKind::Development
        );
        assert_eq!(
            DependencyKind::from_metadata(Cargo::Build),
            DependencyKind::Build
        );
        assert_eq!(
            DependencyKind::from_metadata(Cargo::Unknown),
            DependencyKind::Normal
        );
    }

    #[test]
    fn dependency_kind_display_is_short() {
        assert_eq!(DependencyKind::Normal.to_string(), "normal");
        assert_eq!(DependencyKind::Development.to_string(), "dev");
        assert_eq!(DependencyKind::Build.to_string(), "build");
    }

    #[test]
    fn load_finds_the_members_of_this_workspace() {
        let members = load().unwrap();
        let member = |name: &str| members.iter().find(|m| m.name == name).unwrap();
        let xtask = member("xtask");
        assert_eq!(xtask.location, CrateLocation::Xtask);
        assert_eq!(
            xtask.dependencies,
            [
                Dependency {
                    name: "php-compiler".to_owned(),
                    kind: DependencyKind::Normal,
                },
                Dependency {
                    name: "php-lexer".to_owned(),
                    kind: DependencyKind::Normal,
                },
                Dependency {
                    name: "php-parser".to_owned(),
                    kind: DependencyKind::Normal,
                },
                Dependency {
                    name: "test-harness".to_owned(),
                    kind: DependencyKind::Normal,
                }
            ]
        );
        let harness = member("test-harness");
        assert_eq!(harness.location, CrateLocation::Crates);
        assert!(harness.dependencies.is_empty());
    }
}
