//! Request-local key/group map.

use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::group::Group;
use crate::key::CacheKey;

/// One group's resolved ids to stored bytes.
type Slot = HashMap<Box<[u8]>, Box<[u8]>>;

/// One request's object cache. Not assumed [`Send`].
///
/// Contract: `contracts/cache/memory.md`.
pub struct Memory {
    cache: HashMap<Group, Slot>,
    global: HashSet<Group>,
    prefix: Box<[u8]>,
    network: bool,
    hits: u64,
    misses: u64,
}

impl Memory {
    /// A single-site bag: keys are stored without a site prefix.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            global: HashSet::new(),
            prefix: Box::from([]),
            network: false,
            hits: 0,
            misses: 0,
        }
    }

    /// A network bag. Non-global groups prefix keys with `{blog_id}:`.
    #[must_use]
    pub fn network(blog_id: u64) -> Self {
        Self {
            cache: HashMap::new(),
            global: HashSet::new(),
            prefix: site_prefix(blog_id),
            network: true,
            hits: 0,
            misses: 0,
        }
    }

    /// Mark `group` as global (no site prefix on a network).
    pub fn add_global(&mut self, group: Group) {
        self.global.insert(group);
    }

    /// Change the current site. No effect on a single-site bag.
    pub fn switch_to(&mut self, blog_id: u64) {
        if self.network {
            self.prefix = site_prefix(blog_id);
        }
    }

    /// Insert `value` when the resolved key is missing.
    #[must_use]
    pub fn add(&mut self, key: &CacheKey, value: &[u8], group: &Group) -> bool {
        if self.contains(key, group) {
            return false;
        }
        self.write(key, value, group);
        true
    }

    /// Write `value`, replacing a hit.
    pub fn set(&mut self, key: &CacheKey, value: &[u8], group: &Group) {
        self.write(key, value, group);
    }

    /// Write `value` only when the resolved key is present.
    #[must_use]
    pub fn replace(&mut self, key: &CacheKey, value: &[u8], group: &Group) -> bool {
        if !self.contains(key, group) {
            return false;
        }
        self.write(key, value, group);
        true
    }

    /// Stored bytes for the resolved key, or `None` on a miss.
    pub fn get(&mut self, key: &CacheKey, group: &Group) -> Option<Box<[u8]>> {
        let found = self
            .slot(group)
            .and_then(|slot| slot.get(&self.id(key, group)))
            .cloned();
        if found.is_some() {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        found
    }

    /// Remove the resolved key.
    #[must_use]
    pub fn delete(&mut self, key: &CacheKey, group: &Group) -> bool {
        let id = self.id(key, group);
        self.cache
            .get_mut(group)
            .is_some_and(|slot| slot.remove(&id).is_some())
    }

    /// Drop every group.
    pub fn flush(&mut self) {
        self.cache.clear();
    }

    /// Drop one group name as given, without the empty/`0` rewrite.
    pub fn flush_group(&mut self, group: &Group) {
        self.cache.remove(group);
    }

    /// Successful gets since this bag was created.
    #[must_use]
    pub const fn hits(&self) -> u64 {
        self.hits
    }

    /// Missed gets since this bag was created.
    #[must_use]
    pub const fn misses(&self) -> u64 {
        self.misses
    }

    fn contains(&self, key: &CacheKey, group: &Group) -> bool {
        self.slot(group)
            .is_some_and(|slot| slot.contains_key(&self.id(key, group)))
    }

    fn write(&mut self, key: &CacheKey, value: &[u8], group: &Group) {
        let id = self.id(key, group);
        self.cache
            .entry(group.clone())
            .or_default()
            .insert(id, value.into());
    }

    fn slot(&self, group: &Group) -> Option<&Slot> {
        self.cache.get(group)
    }

    fn id(&self, key: &CacheKey, group: &Group) -> Box<[u8]> {
        if self.network && !self.global.contains(group) {
            let key = key.as_bytes();
            let mut id = Vec::with_capacity(self.prefix.len() + key.len());
            id.extend_from_slice(&self.prefix);
            id.extend_from_slice(key);
            id.into()
        } else {
            key.as_bytes().into()
        }
    }
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Memory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Memory")
            .field("network", &self.network)
            .field("hits", &self.hits)
            .field("misses", &self.misses)
            .finish_non_exhaustive()
    }
}

fn site_prefix(blog_id: u64) -> Box<[u8]> {
    let mut prefix = blog_id.to_string().into_bytes();
    prefix.push(b':');
    prefix.into()
}

#[cfg(test)]
mod tests {
    use super::site_prefix;

    #[test]
    fn site_prefix_is_decimal_and_colon() {
        assert_eq!(site_prefix(1).as_ref(), b"1:");
        assert_eq!(site_prefix(0).as_ref(), b"0:");
        assert_eq!(site_prefix(12).as_ref(), b"12:");
    }
}
