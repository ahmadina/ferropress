//! Role table from the `{prefix}user_roles` option, and `allcaps` merge.

use std::fmt;

use cms_db::{Store, TablePrefix};
use cms_options::{OptionName, Options};
use php_serialize::{Key, Value, decode};

use crate::error::Error;
use crate::grants::{GrantMap, grants_from_value};
use crate::map::Flags;

/// Roles listed in the `user_roles` option.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleTable {
    roles: Vec<(Box<[u8]>, GrantMap)>,
}

impl RoleTable {
    /// No roles.
    #[must_use]
    pub const fn empty() -> Self {
        Self { roles: Vec::new() }
    }

    /// Whether `name` is a listed role.
    #[must_use]
    pub fn is_role(&self, name: &[u8]) -> bool {
        self.roles.iter().any(|(stored, _)| stored.as_ref() == name)
    }

    /// Capabilities for `name`, or `None` when it is not a role.
    #[must_use]
    pub fn caps(&self, name: &[u8]) -> Option<&GrantMap> {
        self.roles
            .iter()
            .rev()
            .find(|(stored, _)| stored.as_ref() == name)
            .map(|(_, caps)| caps)
    }

    /// Whether no roles were stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.roles.is_empty()
    }
}

/// Merge role capabilities with a user's stored map (`WP_User::get_role_caps`).
///
/// Every stored-map key that is a role contributes that role's caps, in stored
/// order. The stored map is then overlaid (later keys win).
#[must_use]
pub fn allcaps(stored: &GrantMap, roles: &RoleTable) -> GrantMap {
    let mut merged = GrantMap::empty();
    for (name, _) in stored.iter() {
        if let Some(role_caps) = roles.caps(name) {
            merged = merged.overlay(role_caps);
        }
    }
    merged.overlay(stored)
}

/// Decode a serialized roles option, or empty when the bytes are not an array.
#[must_use]
pub(crate) fn roles_from_bytes(bytes: &[u8]) -> RoleTable {
    let Ok(document) = decode(bytes) else {
        return RoleTable::empty();
    };
    let Value::Array(entries) = document.root() else {
        return RoleTable::empty();
    };
    let mut roles = Vec::new();
    for (key, value) in entries {
        let Key::String(name) = key else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        roles.push((Box::from(name), role_caps(&value)));
    }
    RoleTable { roles }
}

fn role_caps(value: &Value<'_, '_>) -> GrantMap {
    let Value::Array(fields) = value else {
        return GrantMap::empty();
    };
    for (key, nested) in fields.clone() {
        if matches!(key, Key::String(b"capabilities")) {
            return grants_from_value(&nested);
        }
    }
    GrantMap::empty()
}

/// Request-local `{prefix}user_roles` option.
///
/// Request-affine: not assumed [`Send`]. Contract: `contracts/auth/roles.md`.
pub struct Roles<S> {
    options: Options<S>,
    name: OptionName,
    links: OptionName,
    loaded: Option<RoleTable>,
    flags: Option<Flags>,
}

impl<S: Store> Roles<S> {
    /// A bag that will read `{prefix}options` for `{prefix}user_roles`.
    ///
    /// # Errors
    ///
    /// [`Error::Table`] when the prefix contains a NUL.
    pub fn new(store: S, prefix: &TablePrefix) -> Result<Self, Error> {
        Ok(Self {
            options: Options::new(store, prefix)?,
            name: role_option(prefix)?,
            links: links_option()?,
            loaded: None,
            flags: None,
        })
    }

    /// The role table. Missing or junk bytes are empty.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn get(&mut self) -> Result<RoleTable, Error> {
        if let Some(table) = &self.loaded {
            return Ok(table.clone());
        }
        let table = match self.options.get(&self.name)? {
            Some(bytes) => roles_from_bytes(&bytes),
            None => RoleTable::empty(),
        };
        self.loaded = Some(table.clone());
        Ok(table)
    }

    /// Site flags from options (`link_manager_enabled`).
    ///
    /// Missing or PHP-empty stored bytes are off. A later read in this request
    /// does not hit the store again.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn flags(&mut self) -> Result<Flags, Error> {
        if let Some(flags) = self.flags {
            return Ok(flags);
        }
        let flags = match self.options.get(&self.links)? {
            Some(bytes) if stored_on(&bytes) => Flags::links(true),
            Some(_) | None => Flags::off(),
        };
        self.flags = Some(flags);
        Ok(flags)
    }
}

impl<S> fmt::Debug for Roles<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Roles")
            .field("name", &String::from_utf8_lossy(self.name.as_bytes()))
            .finish_non_exhaustive()
    }
}

fn links_option() -> Result<OptionName, Error> {
    match OptionName::new("link_manager_enabled") {
        Ok(name) => Ok(name),
        Err(_empty) => Err(Error::Unavailable),
    }
}

fn stored_on(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes != b"0"
}

fn role_option(prefix: &TablePrefix) -> Result<OptionName, Error> {
    let mut bytes = Vec::from(prefix.as_bytes());
    bytes.extend_from_slice(b"user_roles");
    match OptionName::new(bytes) {
        Ok(name) => Ok(name),
        Err(_) => match OptionName::new("user_roles") {
            Ok(name) => Ok(name),
            Err(_empty) => Err(Error::Unavailable),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{RoleTable, allcaps, roles_from_bytes};
    use crate::grants::{GrantMap, grants_from_bytes};
    use php_serialize::Writer;

    fn admin_roles() -> Vec<u8> {
        let mut writer = Writer::new();
        writer.array(1);
        writer.string(b"administrator");
        writer.array(2);
        writer.string(b"name");
        writer.string(b"Administrator");
        writer.string(b"capabilities");
        writer.array(2);
        writer.string(b"manage_options");
        writer.bool(true);
        writer.string(b"edit_posts");
        writer.bool(true);
        writer.end();
        writer.end();
        writer.end();
        writer.into_bytes()
    }

    #[test]
    fn merge_overlays_role_caps_then_stored() {
        let roles = roles_from_bytes(&admin_roles());
        assert!(roles.is_role(b"administrator"));
        assert!(!roles.is_role(b"editor"));
        assert!(
            roles
                .caps(b"administrator")
                .is_some_and(|caps| caps.granted(b"manage_options"))
        );
        assert!(RoleTable::empty().is_empty());

        let stored =
            grants_from_bytes(b"a:2:{s:13:\"administrator\";b:1;s:10:\"edit_posts\";b:0;}");
        let merged = allcaps(&stored, &roles);
        assert!(merged.granted(b"manage_options"));
        assert_eq!(merged.get(b"edit_posts"), Some(false));
        assert!(merged.granted(b"administrator"));

        let denied_role = grants_from_bytes(b"a:1:{s:13:\"administrator\";b:0;}");
        let still = allcaps(&denied_role, &roles);
        assert!(still.granted(b"manage_options"));
        assert_eq!(still.get(b"administrator"), Some(false));

        let unknown = grants_from_bytes(b"a:1:{s:6:\"custom\";b:1;}");
        let only = allcaps(&unknown, &roles);
        assert!(only.granted(b"custom"));
        assert!(!only.granted(b"manage_options"));

        assert!(roles_from_bytes(b"").is_empty());
        assert!(roles_from_bytes(b"N;").is_empty());
        assert!(roles_from_bytes(b"not-roles").is_empty());
        let junk_role = roles_from_bytes(b"a:1:{s:6:\"editor\";s:1:\"x\";}");
        assert!(junk_role.is_role(b"editor"));
        assert!(junk_role.caps(b"editor").is_some_and(GrantMap::is_empty));
    }
}
