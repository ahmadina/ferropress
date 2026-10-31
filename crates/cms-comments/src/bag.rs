//! Request-local comment hits on a [`cms_db::Store`].

use std::collections::HashMap;
use std::fmt;

use cms_db::{Reply, ResultSet, Sql, Store, TableName, TablePrefix, TableStem};

use crate::error::Error;
use crate::id::CommentId;
use crate::row::{Comment, from_set};
use crate::sql::select_row;

/// Comments table stem from `contracts/inventory/tables.jsonl`.
const STEM: &[u8] = b"comments";

/// One site's comment rows, loaded through [`Store`].
///
/// Request-affine: not assumed [`Send`]. Contract: `contracts/comments/get.md`.
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

impl<S> fmt::Debug for Comments<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Comments")
            .field("table", &self.table)
            .finish_non_exhaustive()
    }
}
