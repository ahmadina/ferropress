//! The reference manifest: parsing, static validation and archive hash verification.

use std::collections::HashSet;
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Manifest location, relative to the repository root.
pub(crate) const MANIFEST_PATH: &str = "contracts/reference-manifest.toml";

const SUPPORTED_SCHEMA: u32 = 1;
const SOURCE_ROOT: &str = "lab/upstream/";
const ARCHIVE_ROOT: &str = "lab/packages/";
const COMMIT_HEX_LEN: usize = 40;
const SHA256_HEX_LEN: usize = 64;
const READ_BUFFER_LEN: usize = 64 * 1024;
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub(crate) schema: u32,
    #[serde(default)]
    pub(crate) source: Vec<Source>,
    #[serde(default)]
    pub(crate) archive: Vec<Archive>,
    #[serde(default)]
    pub(crate) image: Vec<Image>,
}

/// An upstream git source checked out into the lab.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Source {
    pub(crate) id: String,
    pub(crate) version: String,
    pub(crate) repository: String,
    pub(crate) tag: String,
    pub(crate) commit: String,
    pub(crate) checkout: String,
}

/// A release archive downloaded into the lab.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Archive {
    pub(crate) id: String,
    pub(crate) version: String,
    pub(crate) url: String,
    pub(crate) sha256: String,
    pub(crate) file: String,
}

/// A container image used by the lab stack.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Image {
    pub(crate) id: String,
    pub(crate) reference: String,
    pub(crate) digest: String,
}

pub(crate) fn parse(text: &str) -> Result<Manifest, toml::de::Error> {
    toml::from_str(text)
}

/// What is wrong with one manifest field.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Issue {
    UnsupportedSchema { found: u32 },
    DuplicateId,
    InvalidId,
    Empty,
    NotHttps,
    NotLowercaseHex { expected_len: usize },
    UnpinnedImage,
    OutsideRoot { root: &'static str },
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema { found } => {
                write!(
                    f,
                    "schema {found} is not supported (expected {SUPPORTED_SCHEMA})"
                )
            }
            Self::DuplicateId => f.write_str("id is used by more than one entry"),
            Self::InvalidId => {
                f.write_str("id must be lowercase ASCII letters, digits and inner hyphens")
            }
            Self::Empty => f.write_str("must not be empty"),
            Self::NotHttps => f.write_str("must be an https:// URL"),
            Self::NotLowercaseHex { expected_len } => {
                write!(f, "must be {expected_len} lowercase hexadecimal characters")
            }
            Self::UnpinnedImage => {
                f.write_str("must name an explicit tag other than `latest`, without a digest")
            }
            Self::OutsideRoot { root } => {
                write!(f, "must be a relative path inside `{root}` without `..`")
            }
        }
    }
}

/// One validation failure, located by entry id and field name.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Problem {
    pub(crate) entry: String,
    pub(crate) field: &'static str,
    pub(crate) issue: Issue,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}`.{}: {}", self.entry, self.field, self.issue)
    }
}

/// Checks every rule that does not need the network or the lab; returns all problems found.
pub(crate) fn validate(manifest: &Manifest) -> Vec<Problem> {
    let mut validator = Validator::default();
    if manifest.schema != SUPPORTED_SCHEMA {
        validator.report(
            "manifest",
            "schema",
            Issue::UnsupportedSchema {
                found: manifest.schema,
            },
        );
    }
    for source in &manifest.source {
        let id = source.id.as_str();
        validator.id(id);
        validator.non_empty(id, "version", &source.version);
        validator.https(id, "repository", &source.repository);
        validator.non_empty(id, "tag", &source.tag);
        validator.hex(id, "commit", &source.commit, COMMIT_HEX_LEN);
        validator.lab_path(id, "checkout", &source.checkout, SOURCE_ROOT);
    }
    for archive in &manifest.archive {
        let id = archive.id.as_str();
        validator.id(id);
        validator.non_empty(id, "version", &archive.version);
        validator.https(id, "url", &archive.url);
        validator.hex(id, "sha256", &archive.sha256, SHA256_HEX_LEN);
        validator.lab_path(id, "file", &archive.file, ARCHIVE_ROOT);
    }
    for image in &manifest.image {
        let id = image.id.as_str();
        validator.id(id);
        if !is_pinned_image(&image.reference) {
            validator.report(id, "reference", Issue::UnpinnedImage);
        }
        let digest_hex = image.digest.strip_prefix("sha256:").unwrap_or("");
        validator.hex(id, "digest", digest_hex, SHA256_HEX_LEN);
    }
    validator.problems
}

#[derive(Default)]
struct Validator {
    seen_ids: HashSet<String>,
    problems: Vec<Problem>,
}

impl Validator {
    fn report(&mut self, entry: &str, field: &'static str, issue: Issue) {
        self.problems.push(Problem {
            entry: entry.to_owned(),
            field,
            issue,
        });
    }

    fn id(&mut self, id: &str) {
        if !is_valid_id(id) {
            self.report(id, "id", Issue::InvalidId);
        }
        if !self.seen_ids.insert(id.to_owned()) {
            self.report(id, "id", Issue::DuplicateId);
        }
    }

    fn non_empty(&mut self, id: &str, field: &'static str, value: &str) {
        if value.trim().is_empty() {
            self.report(id, field, Issue::Empty);
        }
    }

    fn https(&mut self, id: &str, field: &'static str, value: &str) {
        if value.strip_prefix("https://").is_none_or(str::is_empty) {
            self.report(id, field, Issue::NotHttps);
        }
    }

    fn hex(&mut self, id: &str, field: &'static str, value: &str, expected_len: usize) {
        if !is_lowercase_hex(value, expected_len) {
            self.report(id, field, Issue::NotLowercaseHex { expected_len });
        }
    }

    fn lab_path(&mut self, id: &str, field: &'static str, value: &str, root: &'static str) {
        if !is_inside_root(value, root) {
            self.report(id, field, Issue::OutsideRoot { root });
        }
    }
}

fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('-')
        && !id.ends_with('-')
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn is_lowercase_hex(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_inside_root(value: &str, root: &str) -> bool {
    value.strip_prefix(root).is_some_and(|rest| {
        !rest.contains('\\')
            && rest
                .split('/')
                .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
    })
}

fn is_pinned_image(reference: &str) -> bool {
    if reference.contains('@') {
        return false;
    }
    let name = reference.rsplit('/').next().unwrap_or(reference);
    match name.split_once(':') {
        Some((repository, tag)) => !repository.is_empty() && !tag.is_empty() && tag != "latest",
        None => false,
    }
}

/// Result of comparing a downloaded archive with its pinned hash.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ArchiveStatus {
    Verified,
    NotFetched,
    Mismatch { actual: String },
}

/// Hashes the archive under `root` if it has been downloaded and compares it with the pinned value.
pub(crate) fn verify_archive(root: &Path, archive: &Archive) -> io::Result<ArchiveStatus> {
    let file = match File::open(root.join(&archive.file)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ArchiveStatus::NotFetched);
        }
        Err(error) => return Err(error),
    };
    let actual = sha256_hex(file)?;
    if actual == archive.sha256 {
        Ok(ArchiveStatus::Verified)
    } else {
        Ok(ArchiveStatus::Mismatch { actual })
    }
}

/// Lowercase hexadecimal SHA-256 of a file's contents, read in bounded chunks.
pub(crate) fn file_sha256(path: &Path) -> io::Result<String> {
    sha256_hex(File::open(path)?)
}

fn sha256_hex(mut reader: impl Read) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; READ_BUFFER_LEN];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => hasher.update(&buffer[..read]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    let mut hex = String::with_capacity(SHA256_HEX_LEN);
    for byte in hasher.finalize() {
        hex.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
        hex.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
    }
    Ok(hex)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    const COMMIT: &str = "ab1bc858a3f02a99c175d9e93c019254a4d5692c";
    /// SHA-256 of the three bytes `abc`.
    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn valid_text() -> String {
        format!(
            r#"
schema = 1

[[source]]
id = "platform"
version = "1.0"
repository = "https://example.org/platform.git"
tag = "1.0"
commit = "{COMMIT}"
checkout = "lab/upstream/platform"

[[archive]]
id = "platform-release"
version = "1.0"
url = "https://example.org/platform-1.0.tar.gz"
sha256 = "{ABC_SHA256}"
file = "lab/packages/platform-1.0.tar.gz"

[[image]]
id = "database"
reference = "docker.io/library/mysql:8.4.13"
digest = "sha256:{ABC_SHA256}"
"#
        )
    }

    fn valid() -> Manifest {
        parse(&valid_text()).unwrap()
    }

    fn issues(manifest: &Manifest) -> Vec<(String, &'static str, Issue)> {
        validate(manifest)
            .into_iter()
            .map(|problem| (problem.entry, problem.field, problem.issue))
            .collect()
    }

    #[test]
    fn committed_manifest_is_valid() {
        let manifest = parse(include_str!("../../contracts/reference-manifest.toml")).unwrap();
        assert_eq!(validate(&manifest), []);
        assert!(!manifest.source.is_empty());
        assert!(!manifest.archive.is_empty());
        assert!(!manifest.image.is_empty());
    }

    #[test]
    fn valid_manifest_has_no_problems() {
        assert_eq!(validate(&valid()), []);
    }

    #[test]
    fn missing_sections_default_to_empty() {
        let manifest = parse("schema = 1").unwrap();
        assert!(manifest.source.is_empty() && manifest.archive.is_empty());
        assert_eq!(validate(&manifest), []);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        assert!(parse("schema = 1\nextra = true").is_err());
        let text = valid_text().replace("tag = \"1.0\"", "tag = \"1.0\"\nbranch = \"main\"");
        assert!(parse(&text).is_err());
    }

    #[test]
    fn unsupported_schema_is_reported() {
        let mut manifest = valid();
        manifest.schema = 2;
        assert_eq!(
            issues(&manifest),
            [(
                "manifest".into(),
                "schema",
                Issue::UnsupportedSchema { found: 2 }
            )]
        );
    }

    #[test]
    fn ids_must_be_unique_and_well_formed() {
        let mut manifest = valid();
        manifest.image[0].id = "platform".into();
        manifest.archive[0].id = "Bad_Id".into();
        assert_eq!(
            issues(&manifest),
            [
                ("Bad_Id".into(), "id", Issue::InvalidId),
                ("platform".into(), "id", Issue::DuplicateId),
            ]
        );
    }

    #[test]
    fn id_rules() {
        for good in ["php", "php-runtime", "a1"] {
            assert!(is_valid_id(good), "{good}");
        }
        for bad in ["", "-a", "a-", "A", "a_b", "a b"] {
            assert!(!is_valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn source_fields_are_checked() {
        let mut manifest = valid();
        let source = &mut manifest.source[0];
        source.version = " ".into();
        source.repository = "http://example.org/x.git".into();
        source.tag = String::new();
        source.commit = COMMIT.to_uppercase();
        source.checkout = "lab/upstream/../../etc".into();
        let problems = issues(&manifest);
        let expected: [(String, &str, Issue); 5] = [
            ("platform".into(), "version", Issue::Empty),
            ("platform".into(), "repository", Issue::NotHttps),
            ("platform".into(), "tag", Issue::Empty),
            (
                "platform".into(),
                "commit",
                Issue::NotLowercaseHex { expected_len: 40 },
            ),
            (
                "platform".into(),
                "checkout",
                Issue::OutsideRoot { root: SOURCE_ROOT },
            ),
        ];
        assert_eq!(problems, expected);
    }

    #[test]
    fn archive_fields_are_checked() {
        let mut manifest = valid();
        let archive = &mut manifest.archive[0];
        archive.version = String::new();
        archive.url = "https://".into();
        archive.sha256 = "abc".into();
        archive.file = "lab/upstream/x.tar.gz".into();
        let expected: [(String, &str, Issue); 4] = [
            ("platform-release".into(), "version", Issue::Empty),
            ("platform-release".into(), "url", Issue::NotHttps),
            (
                "platform-release".into(),
                "sha256",
                Issue::NotLowercaseHex { expected_len: 64 },
            ),
            (
                "platform-release".into(),
                "file",
                Issue::OutsideRoot { root: ARCHIVE_ROOT },
            ),
        ];
        assert_eq!(issues(&manifest), expected);
    }

    #[test]
    fn image_fields_are_checked() {
        let mut manifest = valid();
        manifest.image[0].reference = "mysql:latest".into();
        manifest.image[0].digest = ABC_SHA256.into();
        let expected: [(String, &str, Issue); 2] = [
            ("database".into(), "reference", Issue::UnpinnedImage),
            (
                "database".into(),
                "digest",
                Issue::NotLowercaseHex { expected_len: 64 },
            ),
        ];
        assert_eq!(issues(&manifest), expected);
    }

    #[test]
    fn image_pinning_rules() {
        for good in [
            "mysql:8.4.13",
            "docker.io/library/php:8.5.12-apache",
            "host:5000/a/b:1",
        ] {
            assert!(is_pinned_image(good), "{good}");
        }
        for bad in [
            "mysql",
            "mysql:latest",
            "mysql:",
            ":8.4",
            "host:5000/a/b",
            "mysql:8.4@sha256:00",
        ] {
            assert!(!is_pinned_image(bad), "{bad}");
        }
    }

    #[test]
    fn path_rules() {
        assert!(is_inside_root("lab/upstream/php", SOURCE_ROOT));
        for bad in [
            "lab/upstream/",
            "lab/upstream",
            "/lab/upstream/php",
            "lab/upstream/./php",
            "lab/upstream/a/../../b",
            "lab/upstream//php",
            "lab/upstream/php/",
            "lab/upstream/a\\..\\b",
            "lab/other/php",
        ] {
            assert!(!is_inside_root(bad, SOURCE_ROOT), "{bad}");
        }
    }

    #[test]
    fn lab_roots_are_git_ignored() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for root in [SOURCE_ROOT, ARCHIVE_ROOT] {
            let status = std::process::Command::new("git")
                .args(["check-ignore", "--quiet", &format!("{root}probe")])
                .current_dir(&repo_root)
                .status()
                .unwrap();
            assert!(status.success(), "{root} must be git-ignored");
        }
    }

    #[test]
    fn issues_and_problems_render_readably() {
        let problem = Problem {
            entry: "php".into(),
            field: "commit",
            issue: Issue::NotLowercaseHex { expected_len: 40 },
        };
        assert_eq!(
            problem.to_string(),
            "`php`.commit: must be 40 lowercase hexadecimal characters"
        );
        let rendered = [
            Issue::UnsupportedSchema { found: 9 },
            Issue::DuplicateId,
            Issue::InvalidId,
            Issue::Empty,
            Issue::NotHttps,
            Issue::UnpinnedImage,
            Issue::OutsideRoot { root: ARCHIVE_ROOT },
        ]
        .map(|issue| issue.to_string());
        assert_eq!(
            rendered,
            [
                "schema 9 is not supported (expected 1)",
                "id is used by more than one entry",
                "id must be lowercase ASCII letters, digits and inner hyphens",
                "must not be empty",
                "must be an https:// URL",
                "must name an explicit tag other than `latest`, without a digest",
                "must be a relative path inside `lab/packages/` without `..`",
            ]
        );
    }

    #[test]
    fn sha256_matches_known_vector_across_reads() {
        assert_eq!(sha256_hex(&b"abc"[..]).unwrap(), ABC_SHA256);
        let large = vec![b'x'; READ_BUFFER_LEN * 2 + 7];
        let chained = (&large[..READ_BUFFER_LEN]).chain(&large[READ_BUFFER_LEN..]);
        assert_eq!(
            sha256_hex(chained).unwrap(),
            sha256_hex(&large[..]).unwrap()
        );
    }

    struct Flaky {
        interrupted: bool,
        fail: bool,
    }

    impl Read for Flaky {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(io::ErrorKind::Interrupted.into());
            }
            if self.fail {
                return Err(io::Error::other("disk gone"));
            }
            let bytes = b"abc";
            buffer[..bytes.len()].copy_from_slice(bytes);
            self.fail = true;
            Ok(bytes.len())
        }
    }

    #[test]
    fn sha256_retries_interrupts_and_propagates_errors() {
        let error = sha256_hex(Flaky {
            interrupted: false,
            fail: false,
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "disk gone");
    }

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("xtask-manifest-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(path.join(ARCHIVE_ROOT)).unwrap();
            Self(path)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn archive_status_reflects_the_file_on_disk() {
        let root = TempRoot::new("status");
        let archive = &valid().archive[0];
        assert_eq!(
            verify_archive(&root.0, archive).unwrap(),
            ArchiveStatus::NotFetched
        );

        let path = root.0.join(&archive.file);
        fs::write(&path, b"abc").unwrap();
        assert_eq!(
            verify_archive(&root.0, archive).unwrap(),
            ArchiveStatus::Verified
        );

        fs::write(&path, b"abd").unwrap();
        let ArchiveStatus::Mismatch { actual } = verify_archive(&root.0, archive).unwrap() else {
            panic!("expected a mismatch");
        };
        assert_eq!(actual.len(), SHA256_HEX_LEN);
        assert_ne!(actual, ABC_SHA256);
    }

    #[test]
    fn archive_open_errors_other_than_missing_propagate() {
        let root = TempRoot::new("dir");
        let archive = &valid().archive[0];
        fs::create_dir_all(root.0.join(&archive.file)).unwrap();
        let outcome = verify_archive(&root.0, archive);
        // Opening a directory fails on some platforms and reading it fails on others.
        assert!(outcome.is_err(), "{outcome:?}");
    }
}
