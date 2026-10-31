//! Request-local stored capability maps on a [`cms_db::Store`].

use std::collections::HashMap;
use std::fmt;

use cms_db::{Store, TablePrefix};
use cms_users::{MetaKey, UserId, UserMeta};

use crate::error::Error;
use crate::grants::{GrantMap, grants_from_bytes};

/// Stored `{prefix}capabilities` maps, loaded through [`UserMeta`].
///
/// Request-affine: not assumed [`Send`]. Contract: `contracts/auth/caps.md`.
pub struct Caps<S> {
    meta: UserMeta<S>,
    key: MetaKey,
    hits: HashMap<UserId, GrantMap>,
}

impl<S: Store> Caps<S> {
    /// A bag that will read `{prefix}usermeta` for `{prefix}capabilities`.
    ///
    /// # Errors
    ///
    /// [`Error::Table`] when the prefix contains a NUL.
    pub fn new(store: S, prefix: &TablePrefix) -> Result<Self, Error> {
        Ok(Self {
            meta: UserMeta::new(store, prefix)?,
            key: MetaKey::capabilities(prefix.as_bytes()),
            hits: HashMap::new(),
        })
    }

    /// The stored map for `id`. Missing or junk bytes are empty.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn get(&mut self, id: UserId) -> Result<GrantMap, Error> {
        if let Some(map) = self.hits.get(&id) {
            return Ok(map.clone());
        }
        let map = match self.meta.first(id, &self.key)? {
            Some(bytes) => grants_from_bytes(bytes),
            None => GrantMap::empty(),
        };
        self.hits.insert(id, map.clone());
        Ok(map)
    }

    /// Whether `name` is present and granted on `id`.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn granted(&mut self, id: UserId, name: &[u8]) -> Result<bool, Error> {
        Ok(self.get(id)?.granted(name))
    }
}

impl<S> fmt::Debug for Caps<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Caps")
            .field("key", &String::from_utf8_lossy(self.key.as_bytes()))
            .finish_non_exhaustive()
    }
}
