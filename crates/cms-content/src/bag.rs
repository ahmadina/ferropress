//! Request-local post hits on a [`cms_db::Store`].

use std::collections::HashMap;
use std::fmt;

use cms_db::{Reply, ResultSet, Sql, Store, TableName, TablePrefix, TableStem};

use crate::error::Error;
use crate::id::PostId;
use crate::post::{Post, from_set};
use crate::sql::select_row;

/// Posts table stem from `contracts/inventory/tables.jsonl`.
const STEM: &[u8] = b"posts";

/// One site's post rows, loaded through [`Store`].
///
/// Request-affine: not assumed [`Send`]. Contract: `contracts/content/get.md`.
pub struct Posts<S> {
    store: S,
    table: TableName,
    hits: HashMap<PostId, Post>,
}

impl<S: Store> Posts<S> {
    /// A bag that will read `{prefix}posts`.
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
    pub fn get(&mut self, id: PostId) -> Result<Option<Post>, Error> {
        if let Some(post) = self.hits.get(&id) {
            return Ok(Some(post.clone()));
        }
        match self.query(&select_row(&self.table, id))? {
            RowsOrFailed::Rows(set) => {
                if let Some(post) = from_set(&set) {
                    self.hits.insert(id, post.clone());
                    Ok(Some(post))
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

impl<S> fmt::Debug for Posts<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Posts")
            .field("table", &self.table)
            .finish_non_exhaustive()
    }
}
