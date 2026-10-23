//! Lifecycle of the reference lab stack (`lab/compose.yaml`), driven by pinned manifest values.

use std::fmt;

use crate::manifest::Manifest;
use crate::process::Invocation;

/// Compose file location, relative to the repository root.
pub(crate) const COMPOSE_FILE: &str = "lab/compose.yaml";
/// Address the lab application is published on.
pub(crate) const APP_URL: &str = "http://127.0.0.1:8080/";

const PHP_IMAGE_ID: &str = "php-runtime";
const DATABASE_IMAGE_ID: &str = "database";
const PLATFORM_ARCHIVE_ID: &str = "platform-release";
/// Inside the app container: the lab installer copied in by `lab/app/Dockerfile`.
const INSTALLER: &str = "/opt/ferropress/install-site.php";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LabAction {
    /// Build and start the stack, waiting until every service is healthy.
    Up,
    /// Stop the stack, keeping the database volume.
    Down,
    /// Stop the stack, delete the database volume, start it again and install the site.
    Reset,
    /// Install the site with fixed settings; a no-op when it is already installed.
    Install,
}

impl LabAction {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Reset => "reset",
            Self::Install => "install",
        }
    }
}

/// A manifest entry the lab needs but cannot find.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MissingEntry {
    kind: &'static str,
    id: &'static str,
}

impl fmt::Display for MissingEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the manifest has no {} with id `{}`", self.kind, self.id)
    }
}

impl std::error::Error for MissingEntry {}

/// Environment that `docker compose` needs to interpolate `lab/compose.yaml`.
pub(crate) fn lab_env(manifest: &Manifest) -> Result<Vec<(&'static str, String)>, MissingEntry> {
    let image = |id: &'static str| {
        manifest
            .image
            .iter()
            .find(|image| image.id == id)
            .map(|image| format!("{}@{}", image.reference, image.digest))
            .ok_or(MissingEntry { kind: "image", id })
    };
    let archive = manifest
        .archive
        .iter()
        .find(|archive| archive.id == PLATFORM_ARCHIVE_ID)
        .ok_or(MissingEntry {
            kind: "archive",
            id: PLATFORM_ARCHIVE_ID,
        })?;
    Ok(vec![
        ("LAB_PHP_IMAGE", image(PHP_IMAGE_ID)?),
        ("LAB_DB_IMAGE", image(DATABASE_IMAGE_ID)?),
        ("LAB_PLATFORM_URL", archive.url.clone()),
        ("LAB_PLATFORM_SHA256", archive.sha256.clone()),
    ])
}

/// A `docker compose` invocation against the lab stack.
pub(crate) fn compose<I, S>(env: &[(&'static str, String)], subcommand: I) -> Invocation
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut invocation = Invocation::new(
        "docker",
        ["compose", "--file", COMPOSE_FILE]
            .into_iter()
            .map(String::from)
            .chain(subcommand.into_iter().map(Into::into)),
    );
    invocation.env = env.to_vec();
    invocation
}

/// The `docker compose` invocations that perform `action`, in order.
pub(crate) fn plan(
    action: LabAction,
    manifest: &Manifest,
) -> Result<Vec<Invocation>, MissingEntry> {
    let env = lab_env(manifest)?;
    let up = || compose(&env, ["up", "--build", "--detach", "--wait"]);
    let install = || {
        compose(
            &env,
            ["exec", "-T", "--user", "www-data", "app", "php", INSTALLER],
        )
    };
    Ok(match action {
        LabAction::Up => vec![up()],
        LabAction::Down => vec![compose(&env, ["down"])],
        LabAction::Reset => vec![compose(&env, ["down", "--volumes"]), up(), install()],
        LabAction::Install => vec![install()],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse;

    const DIGEST: &str = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const INSTALL: &str = "docker compose --file lab/compose.yaml exec -T --user www-data app php \
                           /opt/ferropress/install-site.php";

    fn manifest() -> Manifest {
        parse(&format!(
            r#"schema = 1

[[archive]]
id = "platform-release"
version = "1.0"
url = "https://example.org/release.tar.gz"
sha256 = "{hash}"
file = "lab/packages/release.tar.gz"

[[image]]
id = "php-runtime"
reference = "docker.io/library/php:1-apache"
digest = "{DIGEST}"

[[image]]
id = "database"
reference = "docker.io/library/mysql:1"
digest = "{DIGEST}"
"#,
            hash = DIGEST.trim_start_matches("sha256:")
        ))
        .unwrap()
    }

    fn rendered(plan: &[Invocation]) -> Vec<String> {
        plan.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn env_carries_pinned_values() {
        let plan = plan(LabAction::Up, &manifest()).unwrap();
        assert_eq!(
            plan[0].env,
            [
                (
                    "LAB_PHP_IMAGE",
                    format!("docker.io/library/php:1-apache@{DIGEST}")
                ),
                (
                    "LAB_DB_IMAGE",
                    format!("docker.io/library/mysql:1@{DIGEST}")
                ),
                (
                    "LAB_PLATFORM_URL",
                    "https://example.org/release.tar.gz".to_owned()
                ),
                (
                    "LAB_PLATFORM_SHA256",
                    DIGEST.trim_start_matches("sha256:").to_owned()
                ),
            ]
        );
    }

    #[test]
    fn actions_map_to_compose_invocations() {
        let manifest = manifest();
        assert_eq!(
            rendered(&plan(LabAction::Up, &manifest).unwrap()),
            ["docker compose --file lab/compose.yaml up --build --detach --wait"]
        );
        assert_eq!(
            rendered(&plan(LabAction::Down, &manifest).unwrap()),
            ["docker compose --file lab/compose.yaml down"]
        );
        assert_eq!(
            rendered(&plan(LabAction::Reset, &manifest).unwrap()),
            [
                "docker compose --file lab/compose.yaml down --volumes",
                "docker compose --file lab/compose.yaml up --build --detach --wait",
                INSTALL,
            ]
        );
        assert_eq!(
            rendered(&plan(LabAction::Install, &manifest).unwrap()),
            [INSTALL]
        );
    }

    #[test]
    fn compose_prefixes_the_lab_file_and_carries_env() {
        let env = [("KEY", "value".to_owned())];
        let invocation = compose(&env, ["ps"]);
        assert_eq!(
            invocation.to_string(),
            "docker compose --file lab/compose.yaml ps"
        );
        assert_eq!(invocation.env, env);
    }

    #[test]
    fn action_names() {
        assert_eq!(
            [
                LabAction::Up,
                LabAction::Down,
                LabAction::Reset,
                LabAction::Install
            ]
            .map(LabAction::name),
            ["up", "down", "reset", "install"]
        );
    }

    #[test]
    fn missing_entries_are_reported() {
        let mut without_archive = manifest();
        without_archive.archive.clear();
        let error = plan(LabAction::Up, &without_archive).unwrap_err();
        assert_eq!(
            error.to_string(),
            "the manifest has no archive with id `platform-release`"
        );

        let mut without_php = manifest();
        without_php.image.retain(|image| image.id != PHP_IMAGE_ID);
        assert_eq!(
            plan(LabAction::Up, &without_php).unwrap_err(),
            MissingEntry {
                kind: "image",
                id: PHP_IMAGE_ID
            }
        );

        let mut without_database = manifest();
        without_database
            .image
            .retain(|image| image.id != DATABASE_IMAGE_ID);
        assert_eq!(
            plan(LabAction::Down, &without_database).unwrap_err(),
            MissingEntry {
                kind: "image",
                id: DATABASE_IMAGE_ID
            }
        );
    }

    #[test]
    fn committed_manifest_has_every_lab_entry() {
        let manifest = parse(include_str!("../../contracts/reference-manifest.toml")).unwrap();
        for action in [
            LabAction::Up,
            LabAction::Down,
            LabAction::Reset,
            LabAction::Install,
        ] {
            assert!(plan(action, &manifest).is_ok(), "{}", action.name());
        }
    }
}
