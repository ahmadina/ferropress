//! Pure plugin load planning.

use std::collections::BTreeSet;

use cms_request::Flags;

use crate::path::PluginFile;

/// Directories used to build the path each load action receives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// Prefix of must-use files, without a trailing slash.
    pub mu_prefix: Box<[u8]>,
    /// Prefix of site and network plugins, without a trailing slash.
    pub plugins_prefix: Box<[u8]>,
    /// Interoperability name of the legacy hacks file at the installation root.
    pub hacks_name: Box<[u8]>,
}

impl Layout {
    /// The lab's content-relative prefixes and the legacy hacks file name.
    #[must_use]
    pub fn content() -> Self {
        Self {
            mu_prefix: Box::from(b"wp-content/mu-plugins".as_slice()),
            plugins_prefix: Box::from(b"wp-content/plugins".as_slice()),
            hacks_name: Box::from(b"my-hacks.php".as_slice()),
        }
    }
}

/// What the planner can see of the filesystem and options.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// File names directly inside the must-use directory.
    pub mu_files: Vec<PluginFile>,
    /// Network-active plugin paths, as stored.
    pub network_active: Vec<PluginFile>,
    /// Site-active plugin paths, as stored.
    pub site_active: Vec<PluginFile>,
    /// Plugin-directory relative paths that exist.
    pub plugins_present: BTreeSet<PluginFile>,
    /// Must-use file names that exist.
    pub mu_present: BTreeSet<PluginFile>,
    /// Paused plugin paths, skipped in recovery mode.
    pub paused: BTreeSet<PluginFile>,
    /// Whether the `hack_file` option is on.
    pub hack_file: bool,
    /// Whether the legacy hacks file exists at the installation root.
    pub hacks_present: bool,
}

/// Which group a planned file belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadKind {
    /// A must-use plugin.
    MustUse,
    /// A network-wide plugin.
    Network,
    /// A site plugin.
    Site,
    /// The legacy hacks file.
    Hacks,
}

/// One file the request will load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadStep {
    /// The group this file belongs to.
    pub kind: LoadKind,
    /// Path relative to that group's directory, or the hacks file name.
    pub file: PluginFile,
    /// Path fired on the load action.
    pub hook_path: Box<[u8]>,
}

/// Re-sorts a site-active list the way activating a plugin does: ascending byte order.
#[must_use]
pub fn sort_active(paths: Vec<PluginFile>) -> Vec<PluginFile> {
    let mut paths = paths;
    paths.sort();
    paths
}

/// The files to load, in order, for these flags and this snapshot.
#[must_use]
pub fn plan(flags: Flags, layout: &Layout, snapshot: &Snapshot) -> Vec<LoadStep> {
    let mut steps = Vec::new();
    steps.extend(must_use(layout, snapshot));
    if flags.network {
        steps.extend(network(flags, layout, snapshot));
    }
    if !flags.installing {
        if snapshot.hack_file && snapshot.hacks_present {
            steps.push(LoadStep {
                kind: LoadKind::Hacks,
                file: PluginFile::new(&layout.hacks_name),
                hook_path: layout.hacks_name.clone(),
            });
        }
        steps.extend(site(flags, layout, snapshot));
    }
    steps
}

fn must_use(layout: &Layout, snapshot: &Snapshot) -> Vec<LoadStep> {
    let mut files: Vec<PluginFile> = snapshot
        .mu_files
        .iter()
        .filter(|file| file.as_bytes().ends_with(b".php") && snapshot.mu_present.contains(*file))
        .cloned()
        .collect();
    files.sort_by_key(|file| join(&layout.mu_prefix, file));
    files
        .into_iter()
        .map(|file| LoadStep {
            hook_path: join(&layout.mu_prefix, &file),
            kind: LoadKind::MustUse,
            file,
        })
        .collect()
}

fn network(flags: Flags, layout: &Layout, snapshot: &Snapshot) -> Vec<LoadStep> {
    let mut files = snapshot.network_active.clone();
    files.sort();
    files
        .into_iter()
        .filter(|file| usable(file, flags, snapshot))
        .map(|file| LoadStep {
            hook_path: join(&layout.plugins_prefix, &file),
            kind: LoadKind::Network,
            file,
        })
        .collect()
}

fn site(flags: Flags, layout: &Layout, snapshot: &Snapshot) -> Vec<LoadStep> {
    let network: BTreeSet<&PluginFile> = if flags.network {
        snapshot.network_active.iter().collect()
    } else {
        BTreeSet::new()
    };
    snapshot
        .site_active
        .iter()
        .filter(|file| !network.contains(file) && usable(file, flags, snapshot))
        .map(|file| LoadStep {
            kind: LoadKind::Site,
            file: file.clone(),
            hook_path: join(&layout.plugins_prefix, file),
        })
        .collect()
}

fn usable(file: &PluginFile, flags: Flags, snapshot: &Snapshot) -> bool {
    file.is_loadable()
        && snapshot.plugins_present.contains(file)
        && !(flags.recovery && snapshot.paused.contains(file))
}

fn join(prefix: &[u8], file: &PluginFile) -> Box<[u8]> {
    let mut path = Vec::with_capacity(prefix.len() + 1 + file.as_bytes().len());
    path.extend_from_slice(prefix);
    path.push(b'/');
    path.extend_from_slice(file.as_bytes());
    path.into()
}
