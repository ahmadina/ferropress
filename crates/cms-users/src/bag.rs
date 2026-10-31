//! Request-local user hits on a [`cms_db::Store`].

use std::collections::HashMap;
use std::fmt;

use cms_db::{Reply, ResultSet, Sql, Store, TableName, TablePrefix, TableStem};

use crate::draft::Draft;
use crate::error::Error;
use crate::id::UserId;
use crate::row::{User, from_set};
use crate::sql::{delete_row, insert_row, select_row, update_row};
use crate::write::Write;

/// Users table stem from `contracts/inventory/tables.jsonl`.
const STEM: &[u8] = b"users";

/// One network's user rows, loaded through [`Store`].
///
/// Request-affine: not assumed [`Send`]. Contracts: `contracts/users/get.md`,
/// `contracts/users/write.md`.
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

    /// Insert the compact cells. The new id comes from `insert_id`.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn insert(&mut self, draft: &Draft) -> Result<Write, Error> {
        match self.exec(&insert_row(&self.table, draft))? {
            Reply::Done { insert_id, .. } => match UserId::new(insert_id) {
                Ok(id) => Ok(Write::Inserted(id)),
                Err(_) => Ok(Write::Unchanged),
            },
            _ => Ok(Write::Unchanged),
        }
    }

    /// Rewrite the compact cells for `id` (not `user_login`).
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn update(&mut self, id: UserId, draft: &Draft) -> Result<Write, Error> {
        let reply = self.exec(&update_row(&self.table, id, draft))?;
        self.hits.remove(&id);
        match reply {
            Reply::Done { affected, .. } if affected > 0 => Ok(Write::Replaced),
            _ => Ok(Write::Unchanged),
        }
    }

    /// Remove the row for `id`.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn delete(&mut self, id: UserId) -> Result<Write, Error> {
        let reply = self.exec(&delete_row(&self.table, id))?;
        self.hits.remove(&id);
        match reply {
            Reply::Done { affected, .. } if affected > 0 => Ok(Write::Removed),
            _ => Ok(Write::Unchanged),
        }
    }

    fn query(&mut self, sql: &Sql) -> Result<RowsOrFailed, Error> {
        match self.store.query(sql) {
            Ok(Reply::Rows(set)) => Ok(RowsOrFailed::Rows(set)),
            Ok(_) => Ok(RowsOrFailed::Failed),
            Err(_) => Err(Error::Unavailable),
        }
    }

    fn exec(&mut self, sql: &Sql) -> Result<Reply, Error> {
        match self.store.query(sql) {
            Ok(reply) => Ok(reply),
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
