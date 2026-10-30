//! Plugin file paths as the load planner stores them.

use std::fmt;

/// A plugin file path using `/` separators, compared as exact bytes.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginFile(Box<[u8]>);

impl PluginFile {
    /// A path with these bytes.
    #[must_use]
    pub fn new(path: impl AsRef<[u8]>) -> Self {
        Self(path.as_ref().into())
    }

    /// The bytes as stored.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Whether the path ends in `.php`, contains no `..`, and is not absolute.
    #[must_use]
    pub fn is_loadable(&self) -> bool {
        let bytes = self.as_bytes();
        ends_with_php(bytes) && !is_absolute(bytes) && !contains_parent(bytes)
    }
}

impl AsRef<[u8]> for PluginFile {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl From<&[u8]> for PluginFile {
    fn from(path: &[u8]) -> Self {
        Self::new(path)
    }
}

impl From<&str> for PluginFile {
    fn from(path: &str) -> Self {
        Self::new(path.as_bytes())
    }
}

impl fmt::Display for PluginFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from_utf8_lossy(&self.0))
    }
}

fn ends_with_php(path: &[u8]) -> bool {
    path.ends_with(b".php")
}

fn is_absolute(path: &[u8]) -> bool {
    path.starts_with(b"/") || path.get(1) == Some(&b':')
}

fn contains_parent(path: &[u8]) -> bool {
    path.windows(2).any(|pair| pair == b"..")
}

#[cfg(test)]
mod tests {
    use super::PluginFile;

    #[test]
    fn loadable_paths_reject_parent_and_absolute() {
        assert!(PluginFile::from("alpha.php").is_loadable());
        assert!(PluginFile::from("beta/beta.php").is_loadable());
        assert!(!PluginFile::from("alpha.txt").is_loadable());
        assert!(!PluginFile::from("../evil.php").is_loadable());
        assert!(!PluginFile::from("/abs.php").is_loadable());
        assert!(!PluginFile::from("C:/abs.php").is_loadable());
        assert_eq!(format!("{}", PluginFile::from("a.php")), "a.php");
    }
}
