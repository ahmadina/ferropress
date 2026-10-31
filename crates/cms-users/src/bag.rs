//! Request-local user hits on a [`cms_db::Store`].

use std::collections::HashMap;
use std::fmt;

use cms_db::{Reply, ResultSet, Sql, Store, TableName, TablePrefix, TableStem};

use crate::error::Error;
use crate::id::UserId;
use crate::row::{User, from_set};
use crate::sql::select_row;

/// Users table stem from `contracts/inventory/tables.jsonl`.
const STEM: &[u8] = b"users";

/// One network's user rows, loaded through [`Store`].
///
/// Request-affine: not assumed [`Send`]. Contract: `contracts/users/get.md`.
pub struct Users<S> {
    store: S,
    table: TableName,
    hits: HashMap<UserId, User>,
}

impl<S: Store> Users<S> {
    /// A bag that will read `{prefix}users`.
    ///
    /// # Errors
    ///
    /// [`Error::Table`] when the prefix contains a NUL.
    pub fn new(store: S, prefix: &TablePrefix) -> Result<Self, Error> {
        let stem = TableStem::new(STEM)?;
        Ok(Self {
            store,
            table: prefix.table(&stem),
            hits: HashMap::new(),
        })
    }

    /// Stored row for `id`, or `None` when the row does not exist.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn get(&mut self, id: UserId) -> Result<Option<User>, Error> {
        if let Some(user) = self.hits.get(&id) {
            return Ok(Some(user.clone()));
        }
        match self.query(&select_row(&self.table, id))? {
            RowsOrFailed::Rows(set) => {
                if let Some(user) = from_set(&set) {
                    self.hits.insert(id, user.clone());
                    Ok(Some(user))
                } else {
                    Ok(None)
                }
            }
            RowsOrFailed::Failed => Ok(None),
        }
    }

    fn query(&mut self, sql: &Sql) -> Result<RowsOrFailed, Error> {
        match self.store.query(sql) {
            Ok(Reply::Rows(set)) => Ok(RowsOrFailed::Rows(set)),
            Ok(_) => Ok(RowsOrFailed::Failed),
            Err(_) => Err(Error::Unavailable),
        }
    }
}

enum RowsOrFailed {
    Rows(ResultSet),
    Failed,
}

impl<S> fmt::Debug for Users<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Users")
            .field("table", &self.table)
            .finish_non_exhaustive()
    }
}
