//! `lab fetch`: download pinned upstream sources and archives into git-ignored lab directories.
//!
//! Every download lands in a `.partial` path first and is moved into place only after its commit or
//! SHA-256 matches the manifest, so an interrupted or tampered download never looks complete.

use std::fs;
use std::io;
use std::path::Path;

use anyhow::Context as _;

use crate::manifest::{self, Archive, Manifest, Source};
use crate::process::{self, Invocation, Runner};

const PARTIAL_SUFFIX: &str = ".partial";

/// Fetches every source and archive in the manifest; returns one status line per entry.
pub(crate) fn fetch(
    repo_root: &Path,
    manifest: &Manifest,
    runner: &mut impl Runner,
) -> anyhow::Result<Vec<String>> {
    let mut lines = Vec::with_capacity(manifest.source.len() + manifest.archive.len());
    for source in &manifest.source {
        lines.push(fetch_source(repo_root, source, runner)?);
    }
    for archive in &manifest.archive {
        lines.push(fetch_archive(repo_root, archive, runner)?);
    }
    Ok(lines)
}

fn head_commit(runner: &mut impl Runner, checkout: &str) -> anyhow::Result<String> {
    process::capture_checked(
        runner,
        &Invocation::new("git", ["-C", checkout, "rev-parse", "HEAD"]),
    )
}

fn remove_dir_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

fn remove_file_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

fn create_parent(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    Ok(())
}

fn fetch_source(
    repo_root: &Path,
    source: &Source,
    runner: &mut impl Runner,
) -> anyhow::Result<String> {
    let checkout = repo_root.join(&source.checkout);
    if checkout.exists() {
        let head = head_commit(runner, &source.checkout)?;
        anyhow::ensure!(
            head == source.commit,
            "source `{}`: {} is at {head}, not the pinned {}; delete it and fetch again",
            source.id,
            source.checkout,
            source.commit
        );
        return Ok(format!(
            "source `{}`: already at pinned commit {}",
            source.id, source.commit
        ));
    }

    let partial = format!("{}{PARTIAL_SUFFIX}", source.checkout);
    let partial_path = repo_root.join(&partial);
    remove_dir_if_present(&partial_path)
        .with_context(|| format!("removing {}", partial_path.display()))?;
    create_parent(&checkout)?;
    process::run_checked(
        runner,
        &Invocation::new(
            "git",
            [
                "-c",
                "advice.detachedHead=false",
                "clone",
                "--quiet",
                "--depth",
                "1",
                "--single-branch",
                "--branch",
                &source.tag,
                &source.repository,
                &partial,
            ],
        ),
    )?;
    let head = head_commit(runner, &partial)?;
    if head != source.commit {
        remove_dir_if_present(&partial_path)
            .with_context(|| format!("removing {}", partial_path.display()))?;
        anyhow::bail!(
            "source `{}`: tag `{}` now resolves to {head}, not the pinned {}; the upstream tag moved",
            source.id,
            source.tag,
            source.commit
        );
    }
    fs::rename(&partial_path, &checkout)
        .with_context(|| format!("moving {} into place", partial_path.display()))?;
    Ok(format!(
        "source `{}`: cloned tag `{}` at pinned commit {}",
        source.id, source.tag, source.commit
    ))
}

fn fetch_archive(
    repo_root: &Path,
    archive: &Archive,
    runner: &mut impl Runner,
) -> anyhow::Result<String> {
    let path = repo_root.join(&archive.file);
    if path.exists() {
        let actual =
            manifest::file_sha256(&path).with_context(|| format!("hashing {}", path.display()))?;
        anyhow::ensure!(
            actual == archive.sha256,
            "archive `{}`: {} hashes to {actual}, not the pinned {}; delete it and fetch again",
            archive.id,
            archive.file,
            archive.sha256
        );
        return Ok(format!(
            "archive `{}`: already downloaded and verified",
            archive.id
        ));
    }

    let partial = format!("{}{PARTIAL_SUFFIX}", archive.file);
    let partial_path = repo_root.join(&partial);
    create_parent(&path)?;
    process::run_checked(
        runner,
        &Invocation::new(
            "curl",
            [
                "--fail",
                "--location",
                "--silent",
                "--show-error",
                "--proto",
                "=https",
                "--output",
                &partial,
                &archive.url,
            ],
        ),
    )?;
    let actual = manifest::file_sha256(&partial_path)
        .with_context(|| format!("hashing {}", partial_path.display()))?;
    if actual != archive.sha256 {
        remove_file_if_present(&partial_path)
            .with_context(|| format!("removing {}", partial_path.display()))?;
        anyhow::bail!(
            "archive `{}`: download hashes to {actual}, not the pinned {}",
            archive.id,
            archive.sha256
        );
    }
    fs::rename(&partial_path, &path)
        .with_context(|| format!("moving {} into place", partial_path.display()))?;
    Ok(format!("archive `{}`: downloaded and verified", archive.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse;
    use crate::process::testing::FakeRunner;
    use std::path::PathBuf;

    const COMMIT: &str = "ab1bc858a3f02a99c175d9e93c019254a4d5692c";
    const OTHER_COMMIT: &str = "0000000000000000000000000000000000000000";
    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn manifest() -> Manifest {
        parse(&format!(
            r#"schema = 1

[[source]]
id = "platform"
version = "1.0"
repository = "https://example.org/platform.git"
tag = "1.0"
commit = "{COMMIT}"
checkout = "lab/upstream/platform"

[[archive]]
id = "release"
version = "1.0"
url = "https://example.org/release.tar.gz"
sha256 = "{ABC_SHA256}"
file = "lab/packages/release.tar.gz"
"#
        ))
        .unwrap()
    }

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("xtask-fetch-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Simulates git and curl against a temporary root: clones create the target directory, `rev-parse`
    /// answers `head`, and downloads write `body`.
    fn simulated(
        root: &Path,
        head: &'static str,
        body: &'static [u8],
    ) -> impl FnMut(&Invocation) -> io::Result<Option<String>> {
        let root = root.to_path_buf();
        move |invocation: &Invocation| {
            let last = invocation.args.last().cloned().unwrap_or_default();
            match (
                invocation.program,
                invocation.args.iter().any(|a| a == "clone"),
            ) {
                ("git", true) => fs::create_dir_all(root.join(&last)).map(|()| Some(String::new())),
                ("git", false) => Ok(Some(format!("{head}\n"))),
                _ => {
                    let output = &invocation.args[invocation.args.len() - 2];
                    fs::write(root.join(output), body).map(|()| Some(String::new()))
                }
            }
        }
    }

    #[test]
    fn fresh_fetch_clones_downloads_and_verifies() {
        let root = TempRoot::new("fresh");
        let mut runner = FakeRunner::new(simulated(&root.0, COMMIT, b"abc"));
        let lines = fetch(&root.0, &manifest(), &mut runner).unwrap();
        assert_eq!(
            lines,
            [
                format!("source `platform`: cloned tag `1.0` at pinned commit {COMMIT}"),
                "archive `release`: downloaded and verified".to_owned(),
            ]
        );
        assert_eq!(
            runner.seen,
            [
                "git -c advice.detachedHead=false clone --quiet --depth 1 --single-branch --branch 1.0 \
                 https://example.org/platform.git lab/upstream/platform.partial",
                "git -C lab/upstream/platform.partial rev-parse HEAD",
                "curl --fail --location --silent --show-error --proto =https --output \
                 lab/packages/release.tar.gz.partial https://example.org/release.tar.gz",
            ]
        );
        assert!(root.0.join("lab/upstream/platform").is_dir());
        assert!(!root.0.join("lab/upstream/platform.partial").exists());
        assert_eq!(
            fs::read(root.0.join("lab/packages/release.tar.gz")).unwrap(),
            b"abc"
        );
    }

    #[test]
    fn second_fetch_only_verifies() {
        let root = TempRoot::new("again");
        fetch(
            &root.0,
            &manifest(),
            &mut FakeRunner::new(simulated(&root.0, COMMIT, b"abc")),
        )
        .unwrap();
        let mut runner = FakeRunner::new(simulated(&root.0, COMMIT, b"abc"));
        let lines = fetch(&root.0, &manifest(), &mut runner).unwrap();
        assert_eq!(
            lines,
            [
                format!("source `platform`: already at pinned commit {COMMIT}"),
                "archive `release`: already downloaded and verified".to_owned(),
            ]
        );
        assert_eq!(runner.seen, ["git -C lab/upstream/platform rev-parse HEAD"]);
    }

    #[test]
    fn stale_partial_checkout_is_replaced() {
        let root = TempRoot::new("stale");
        let stale = root.0.join("lab/upstream/platform.partial/leftover");
        fs::create_dir_all(&stale).unwrap();
        let mut only_source = manifest();
        only_source.archive.clear();
        fetch(
            &root.0,
            &only_source,
            &mut FakeRunner::new(simulated(&root.0, COMMIT, b"")),
        )
        .unwrap();
        assert!(!root.0.join("lab/upstream/platform/leftover").exists());
    }

    #[test]
    fn existing_checkout_at_another_commit_is_refused() {
        let root = TempRoot::new("drift");
        fs::create_dir_all(root.0.join("lab/upstream/platform")).unwrap();
        let mut runner = FakeRunner::new(simulated(&root.0, OTHER_COMMIT, b"abc"));
        let error = fetch(&root.0, &manifest(), &mut runner)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(&format!("is at {OTHER_COMMIT}, not the pinned {COMMIT}")),
            "{error}"
        );
    }

    #[test]
    fn moved_upstream_tag_is_rejected_and_cleaned_up() {
        let root = TempRoot::new("moved");
        let mut runner = FakeRunner::new(simulated(&root.0, OTHER_COMMIT, b"abc"));
        let error = fetch(&root.0, &manifest(), &mut runner)
            .unwrap_err()
            .to_string();
        assert!(error.ends_with("the upstream tag moved"), "{error}");
        assert!(!root.0.join("lab/upstream/platform.partial").exists());
        assert!(!root.0.join("lab/upstream/platform").exists());
    }

    #[test]
    fn tampered_download_is_rejected_and_cleaned_up() {
        let root = TempRoot::new("tampered");
        let mut only_archive = manifest();
        only_archive.source.clear();
        let mut runner = FakeRunner::new(simulated(&root.0, COMMIT, b"evil"));
        let error = fetch(&root.0, &only_archive, &mut runner)
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("archive `release`: download hashes to "),
            "{error}"
        );
        assert!(!root.0.join("lab/packages/release.tar.gz.partial").exists());
        assert!(!root.0.join("lab/packages/release.tar.gz").exists());
    }

    #[test]
    fn existing_archive_with_wrong_hash_is_refused() {
        let root = TempRoot::new("bad-archive");
        let mut only_archive = manifest();
        only_archive.source.clear();
        fs::create_dir_all(root.0.join("lab/packages")).unwrap();
        fs::write(root.0.join("lab/packages/release.tar.gz"), b"old").unwrap();
        let mut runner = FakeRunner::new(simulated(&root.0, COMMIT, b"abc"));
        let error = fetch(&root.0, &only_archive, &mut runner)
            .unwrap_err()
            .to_string();
        assert!(error.ends_with("delete it and fetch again"), "{error}");
        assert!(runner.seen.is_empty());
    }

    #[test]
    fn failed_commands_propagate() {
        let root = TempRoot::new("failing");
        let mut runner = FakeRunner::new(|_: &Invocation| Ok(None));
        let error = fetch(&root.0, &manifest(), &mut runner)
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("`git -c advice.detachedHead=false clone"),
            "{error}"
        );

        let mut only_archive = manifest();
        only_archive.source.clear();
        let error = fetch(&root.0, &only_archive, &mut runner)
            .unwrap_err()
            .to_string();
        assert!(error.starts_with("`curl --fail"), "{error}");
    }

    #[test]
    fn download_that_writes_nothing_is_reported() {
        let root = TempRoot::new("empty-download");
        let mut only_archive = manifest();
        only_archive.source.clear();
        let mut runner = FakeRunner::new(|_: &Invocation| Ok(Some(String::new())));
        let error = fetch(&root.0, &only_archive, &mut runner).unwrap_err();
        assert!(format!("{error:#}").starts_with("hashing "), "{error:#}");
    }

    #[test]
    fn removal_helpers_ignore_missing_paths_only() {
        let root = TempRoot::new("remove");
        assert!(remove_dir_if_present(&root.0.join("absent")).is_ok());
        assert!(remove_file_if_present(&root.0.join("absent")).is_ok());
        let file = root.0.join("file");
        fs::write(&file, b"x").unwrap();
        assert!(remove_dir_if_present(&file).is_err());
        assert!(remove_file_if_present(&root.0).is_err());
    }
}
