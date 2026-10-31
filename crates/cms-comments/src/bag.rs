//! Request-local comment hits on a [`cms_db::Store`].

use std::collections::HashMap;
use std::fmt;

use cms_db::{Reply, ResultSet, Sql, Store, TableName, TablePrefix, TableStem};

use crate::draft::Draft;
use crate::error::Error;
use crate::id::CommentId;
use crate::row::{Comment, from_set};
use crate::sql::{delete_row, insert_row, select_row, update_row};
use crate::write::Write;

/// Comments table stem from `contracts/inventory/tables.jsonl`.
const STEM: &[u8] = b"comments";

/// One site's comment rows, loaded through [`Store`].
///
/// Request-affine: not assumed [`Send`]. Contracts: `contracts/comments/get.md`,
/// `contracts/comments/write.md`.
pub struct Comments<S> {
    store: S,
    table: TableName,
    hits: HashMap<CommentId, Comment>,
}

impl<S: Store> Comments<S> {
    /// A bag that will read `{prefix}comments`.
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
    pub fn get(&mut self, id: CommentId) -> Result<Option<Comment>, Error> {
        if let Some(comment) = self.hits.get(&id) {
            return Ok(Some(comment.clone()));
        }
        match self.query(&select_row(&self.table, id))? {
            RowsOrFailed::Rows(set) => {
                if let Some(comment) = from_set(&set) {
                    self.hits.insert(id, comment.clone());
                    Ok(Some(comment))
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
            Reply::Done { insert_id, .. } => match CommentId::new(insert_id) {
                Ok(id) => Ok(Write::Inserted(id)),
                Err(_) => Ok(Write::Unchanged),
            },
            _ => Ok(Write::Unchanged),
        }
    }

    /// Rewrite the compact cells for `id`.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn update(&mut self, id: CommentId, draft: &Draft) -> Result<Write, Error> {
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
    pub fn delete(&mut self, id: CommentId) -> Result<Write, Error> {
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

impl<S> fmt::Debug for Comments<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Comments")
            .field("table", &self.table)
            .finish_non_exhaustive()
    }
}
