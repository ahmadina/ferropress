//! Request-local current user loaded through [`cms_users::Users`].

use std::fmt;

use cms_db::{Store, TablePrefix};
use cms_users::{User, UserId, Users};

use crate::caps::Caps;
use crate::error::Error;
use crate::grants::GrantMap;
use crate::map::{Actor, map_user};
use crate::roles::RoleTable;

/// The user that is current for this request, if any.
///
/// Request-affine: not assumed [`Send`]. Contract: `contracts/auth/current.md`.
pub struct Current<S> {
    users: Users<S>,
    loaded: Option<User>,
}

impl<S: Store> Current<S> {
    /// An unset current user that will load `{prefix}users`.
    ///
    /// # Errors
    ///
    /// [`Error::Table`] when the prefix contains a NUL.
    pub fn new(store: S, prefix: &TablePrefix) -> Result<Self, Error> {
        Ok(Self {
            users: Users::new(store, prefix)?,
            loaded: None,
        })
    }

    /// The current row, or `None` when unset or empty.
    #[must_use]
    pub fn get(&self) -> Option<&User> {
        self.loaded.as_ref()
    }

    /// The current id, or `None` when unset or empty.
    #[must_use]
    pub fn id(&self) -> Option<UserId> {
        self.loaded.as_ref().map(User::id)
    }

    /// Load `id` and make it current.
    ///
    /// A miss leaves the current user empty. Setting the same id that is already
    /// current does not hit the store.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn set(&mut self, id: UserId) -> Result<Option<&User>, Error> {
        if self.loaded.as_ref().is_some_and(|user| user.id() == id) {
            return Ok(self.loaded.as_ref());
        }
        self.loaded = self.users.get(id)?;
        Ok(self.loaded.as_ref())
    }

    /// Forget the current user. Does not hit the store.
    pub fn clear(&mut self) {
        self.loaded = None;
    }

    /// Whether the current user is allowed `cap` (`contracts/auth/current-can.md`).
    ///
    /// Unset or empty uses the anonymous actor and an empty map, without
    /// reading usermeta.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when a loaded user cannot read the caps store.
    pub fn can<C: Store>(
        &self,
        caps: &mut Caps<C>,
        roles: &RoleTable,
        cap: &[u8],
    ) -> Result<bool, Error> {
        self.can_user(caps, roles, cap, None)
    }

    /// Whether the current user is allowed `cap` against a user object.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when a loaded user cannot read the caps store.
    pub fn can_user<C: Store>(
        &self,
        caps: &mut Caps<C>,
        roles: &RoleTable,
        cap: &[u8],
        target: Option<UserId>,
    ) -> Result<bool, Error> {
        if let Some(id) = self.id() {
            caps.can_user(id, roles, cap, target)
        } else {
            let mapped = map_user(cap, Actor::Anonymous, target);
            let required: Vec<&[u8]> = mapped.names().collect();
            Ok(GrantMap::empty().has(&required))
        }
    }
}

impl<S> fmt::Debug for Current<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Current")
            .field("id", &self.loaded.as_ref().map(User::id))
            .finish_non_exhaustive()
    }
}
