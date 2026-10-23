//! Workspace layer model and the dependency directions it permits.
//!
//! Dependencies point inward: binaries → platform/infrastructure/bindings → orchestration →
//! domain → foundation, with the PHP runtime depending only on foundation. Same-layer
//! dependencies are allowed where noted; Cargo itself rejects cycles.

use std::collections::HashMap;
use std::fmt;

use crate::workspace::{CrateLocation, DependencyKind, Member};

/// Architectural layer of a workspace crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layer {
    Foundation,
    Domain,
    Orchestration,
    PhpRuntime,
    PhpBindings,
    Infrastructure,
    Platform,
    TestSupport,
    Binary,
}

const FOUNDATION_CRATES: &[&str] = &["cms-types", "php-value", "php-serialize", "cms-contracts"];
const ORCHESTRATION_CRATES: &[&str] = &["cms-request", "cms-bootstrap"];
const BINDINGS_CRATES: &[&str] = &["php-bindings"];
const TEST_SUPPORT_CRATES: &[&str] = &["test-harness"];

impl Layer {
    /// Classifies a crate by its location and name, or `None` if it fits no layer.
    pub(crate) fn classify(name: &str, location: CrateLocation) -> Option<Self> {
        match location {
            CrateLocation::Apps | CrateLocation::Xtask => Some(Self::Binary),
            CrateLocation::Other => None,
            CrateLocation::Crates => Self::classify_library(name),
        }
    }

    fn classify_library(name: &str) -> Option<Self> {
        let layer = if FOUNDATION_CRATES.contains(&name) {
            Self::Foundation
        } else if ORCHESTRATION_CRATES.contains(&name) {
            Self::Orchestration
        } else if BINDINGS_CRATES.contains(&name) {
            Self::PhpBindings
        } else if TEST_SUPPORT_CRATES.contains(&name) {
            Self::TestSupport
        } else if name.starts_with("php-") {
            Self::PhpRuntime
        } else if name.starts_with("cms-") {
            Self::Domain
        } else if name.starts_with("infra-") {
            Self::Infrastructure
        } else if name.starts_with("platform-") {
            Self::Platform
        } else {
            return None;
        };
        Some(layer)
    }

    /// Whether a crate in this layer may depend on a crate in `target` with the given kind.
    pub(crate) fn permits(self, target: Self, kind: DependencyKind) -> bool {
        use Layer::{
            Binary, Domain, Foundation, Infrastructure, Orchestration, PhpBindings, PhpRuntime,
            Platform, TestSupport,
        };

        if target == Binary {
            return false;
        }
        if target == TestSupport {
            return matches!(self, Binary | TestSupport) || kind == DependencyKind::Development;
        }
        match self {
            Binary | TestSupport => true,
            Foundation => target == Foundation,
            Domain => matches!(target, Foundation | Domain),
            Orchestration => matches!(target, Foundation | Domain | Orchestration),
            PhpRuntime => matches!(target, Foundation | PhpRuntime),
            PhpBindings => matches!(
                target,
                Foundation | Domain | Orchestration | PhpRuntime | PhpBindings
            ),
            Infrastructure => matches!(target, Foundation | Domain),
            Platform => matches!(target, Foundation | Domain | Orchestration | Platform),
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Foundation => "foundation",
            Self::Domain => "domain",
            Self::Orchestration => "orchestration",
            Self::PhpRuntime => "php-runtime",
            Self::PhpBindings => "php-bindings",
            Self::Infrastructure => "infrastructure",
            Self::Platform => "platform",
            Self::TestSupport => "test-support",
            Self::Binary => "binary",
        }
    }
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A violation of the layer rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Finding {
    /// A crate whose location or name matches no layer.
    Unclassified { krate: String },
    /// A dependency that points in a forbidden direction.
    ForbiddenDependency {
        from: String,
        from_layer: Layer,
        to: String,
        to_layer: Layer,
        kind: DependencyKind,
    },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unclassified { krate } => write!(
                f,
                "crate `{krate}` fits no layer; place it under crates/ with a layer prefix \
                 (cms-, php-, infra-, platform-) or under apps/"
            ),
            Self::ForbiddenDependency {
                from,
                from_layer,
                to,
                to_layer,
                kind,
            } => write!(
                f,
                "`{from}` ({from_layer}) must not depend on `{to}` ({to_layer}) as a {kind} dependency"
            ),
        }
    }
}

/// Checks every member and dependency edge against the layer rules.
pub(crate) fn check(members: &[Member]) -> Vec<Finding> {
    let layers: HashMap<&str, Layer> = members
        .iter()
        .filter_map(|member| {
            Layer::classify(&member.name, member.location)
                .map(|layer| (member.name.as_str(), layer))
        })
        .collect();

    let mut findings = Vec::new();
    for member in members {
        let Some(&from_layer) = layers.get(member.name.as_str()) else {
            findings.push(Finding::Unclassified {
                krate: member.name.clone(),
            });
            continue;
        };
        for dependency in &member.dependencies {
            let Some(&to_layer) = layers.get(dependency.name.as_str()) else {
                continue;
            };
            if !from_layer.permits(to_layer, dependency.kind) {
                findings.push(Finding::ForbiddenDependency {
                    from: member.name.clone(),
                    from_layer,
                    to: dependency.name.clone(),
                    to_layer,
                    kind: dependency.kind,
                });
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Dependency;
    use DependencyKind::{Build, Development, Normal};
    use Layer::{
        Binary, Domain, Foundation, Infrastructure, Orchestration, PhpBindings, PhpRuntime,
        Platform, TestSupport,
    };

    const ALL: [Layer; 9] = [
        Foundation,
        Domain,
        Orchestration,
        PhpRuntime,
        PhpBindings,
        Infrastructure,
        Platform,
        TestSupport,
        Binary,
    ];

    fn library(name: &str, dependencies: &[(&str, DependencyKind)]) -> Member {
        member(name, CrateLocation::Crates, dependencies)
    }

    fn member(
        name: &str,
        location: CrateLocation,
        dependencies: &[(&str, DependencyKind)],
    ) -> Member {
        Member {
            name: name.to_owned(),
            location,
            dependencies: dependencies
                .iter()
                .map(|&(name, kind)| Dependency {
                    name: name.to_owned(),
                    kind,
                })
                .collect(),
        }
    }

    #[test]
    fn classifies_named_and_prefixed_library_crates() {
        let cases = [
            ("cms-types", Foundation),
            ("php-value", Foundation),
            ("php-serialize", Foundation),
            ("cms-contracts", Foundation),
            ("cms-request", Orchestration),
            ("cms-bootstrap", Orchestration),
            ("php-bindings", PhpBindings),
            ("test-harness", TestSupport),
            ("php-vm-adapter", PhpRuntime),
            ("php-abi-bridge", PhpRuntime),
            ("cms-hooks", Domain),
            ("infra-mysql", Infrastructure),
            ("platform-server", Platform),
        ];
        for (name, expected) in cases {
            assert_eq!(
                Layer::classify(name, CrateLocation::Crates),
                Some(expected),
                "{name}"
            );
        }
    }

    #[test]
    fn binaries_are_classified_by_location_regardless_of_name() {
        assert_eq!(Layer::classify("server", CrateLocation::Apps), Some(Binary));
        assert_eq!(
            Layer::classify("cms-hooks", CrateLocation::Apps),
            Some(Binary)
        );
        assert_eq!(Layer::classify("xtask", CrateLocation::Xtask), Some(Binary));
    }

    #[test]
    fn rejects_unprefixed_or_misplaced_crates() {
        assert_eq!(Layer::classify("utils", CrateLocation::Crates), None);
        assert_eq!(Layer::classify("common", CrateLocation::Crates), None);
        assert_eq!(Layer::classify("cms-hooks", CrateLocation::Other), None);
    }

    #[test]
    fn permitted_normal_dependencies_match_the_layer_table() {
        let allowed: &[(Layer, &[Layer])] = &[
            (Foundation, &[Foundation]),
            (Domain, &[Foundation, Domain]),
            (Orchestration, &[Foundation, Domain, Orchestration]),
            (PhpRuntime, &[Foundation, PhpRuntime]),
            (
                PhpBindings,
                &[Foundation, Domain, Orchestration, PhpRuntime, PhpBindings],
            ),
            (Infrastructure, &[Foundation, Domain]),
            (Platform, &[Foundation, Domain, Orchestration, Platform]),
            (
                TestSupport,
                &[
                    Foundation,
                    Domain,
                    Orchestration,
                    PhpRuntime,
                    PhpBindings,
                    Infrastructure,
                    Platform,
                    TestSupport,
                ],
            ),
            (
                Binary,
                &[
                    Foundation,
                    Domain,
                    Orchestration,
                    PhpRuntime,
                    PhpBindings,
                    Infrastructure,
                    Platform,
                    TestSupport,
                ],
            ),
        ];
        for &(from, targets) in allowed {
            for to in ALL {
                for kind in [Normal, Build] {
                    assert_eq!(
                        from.permits(to, kind),
                        targets.contains(&to),
                        "{from} -> {to} ({kind})"
                    );
                }
            }
        }
    }

    #[test]
    fn dev_dependencies_additionally_allow_test_support_only() {
        for from in ALL {
            assert!(from.permits(TestSupport, Development), "{from}");
            assert!(!from.permits(Binary, Development), "{from}");
            for to in ALL {
                if to != TestSupport {
                    assert_eq!(
                        from.permits(to, Development),
                        from.permits(to, Normal),
                        "{from} -> {to}"
                    );
                }
            }
        }
    }

    #[test]
    fn layer_labels_are_kebab_case() {
        let labels: Vec<String> = ALL.iter().map(ToString::to_string).collect();
        assert_eq!(
            labels,
            [
                "foundation",
                "domain",
                "orchestration",
                "php-runtime",
                "php-bindings",
                "infrastructure",
                "platform",
                "test-support",
                "binary"
            ]
        );
    }

    #[test]
    fn clean_graph_has_no_findings() {
        let members = [
            library("cms-types", &[]),
            library(
                "cms-hooks",
                &[("cms-types", Normal), ("test-harness", Development)],
            ),
            library("test-harness", &[("cms-hooks", Normal)]),
            member("server", CrateLocation::Apps, &[("cms-hooks", Normal)]),
        ];
        assert!(check(&members).is_empty());
    }

    #[test]
    fn reports_inward_violation_with_both_layers() {
        let members = [
            library("cms-hooks", &[("infra-mysql", Normal)]),
            library("infra-mysql", &[]),
        ];
        let findings = check(&members);
        assert_eq!(
            findings,
            [Finding::ForbiddenDependency {
                from: "cms-hooks".into(),
                from_layer: Domain,
                to: "infra-mysql".into(),
                to_layer: Infrastructure,
                kind: Normal,
            }]
        );
        assert_eq!(
            findings[0].to_string(),
            "`cms-hooks` (domain) must not depend on `infra-mysql` (infrastructure) as a normal dependency"
        );
    }

    #[test]
    fn reports_test_support_as_normal_dependency_of_library() {
        let members = [
            library("cms-hooks", &[("test-harness", Normal)]),
            library("test-harness", &[]),
        ];
        assert_eq!(check(&members).len(), 1);
    }

    #[test]
    fn reports_unclassified_crate_once_and_skips_edges_to_it() {
        let members = [
            library("utils", &[]),
            library("cms-hooks", &[("utils", Normal)]),
        ];
        let findings = check(&members);
        assert_eq!(
            findings,
            [Finding::Unclassified {
                krate: "utils".into()
            }]
        );
        assert!(
            findings[0]
                .to_string()
                .starts_with("crate `utils` fits no layer")
        );
    }
}
