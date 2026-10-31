//! Request-local term hits on a [`cms_db::Store`].

use std::collections::HashMap;
use std::fmt;

use cms_db::{Reply, ResultSet, Sql, Store, TableName, TablePrefix, TableStem};

use crate::error::Error;
use crate::found::Found;
use crate::id::TermId;
use crate::sql::select_rows;
use crate::term::{Term, from_set};

/// Terms table stem from `contracts/inventory/tables.jsonl`.
const TERMS: &[u8] = b"terms";
/// Term taxonomy table stem from `contracts/inventory/tables.jsonl`.
const TAXONOMY: &[u8] = b"term_taxonomy";

/// One site's term rows, loaded through [`Store`].
///
/// Request-affine: not assumed [`Send`]. Contract: `contracts/taxonomy/get.md`.
pub struct Terms<S> {
    store: S,
    term_table: TableName,
    taxonomy_table: TableName,
    hits: HashMap<TermId, Term>,
}

impl<S: Store> Terms<S> {
    /// A bag that will read `{prefix}terms` joined to `{prefix}term_taxonomy`.
    ///
    /// # Errors
    ///
    /// [`Error::Table`] when the prefix contains a NUL.
    pub fn new(store: S, prefix: &TablePrefix) -> Result<Self, Error> {
        let terms = TableStem::new(TERMS)?;
        let taxonomy = TableStem::new(TAXONOMY)?;
        Ok(Self {
            store,
            term_table: prefix.table(&terms),
            taxonomy_table: prefix.table(&taxonomy),
            hits: HashMap::new(),
        })
    }

    /// Joined row for `id`, optionally limited to `taxonomy`.
    ///
    /// An empty `taxonomy` matches any join row. More than one usable row with
    /// an empty filter is [`Found::Shared`].
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn get(&mut self, id: TermId, taxonomy: &[u8]) -> Result<Found, Error> {
        if let Some(term) = self.hits.get(&id)
            && (taxonomy.is_empty() || term.taxonomy() == taxonomy)
        {
            return Ok(Found::One(term.clone()));
        }
        match self.query(&select_rows(&self.term_table, &self.taxonomy_table, id))? {
            RowsOrFailed::Rows(set) => Ok(self.pick(id, taxonomy, from_set(&set))),
            RowsOrFailed::Failed => Ok(Found::Miss),
        }
    }

    fn pick(&mut self, id: TermId, taxonomy: &[u8], rows: Vec<Term>) -> Found {
        let unique = rows.len() == 1;
        let chosen = if taxonomy.is_empty() {
            match rows.len() {
                0 => return Found::Miss,
                1 => rows.into_iter().next(),
                _ => return Found::Shared,
            }
        } else {
            rows.into_iter().find(|term| term.taxonomy() == taxonomy)
        };
        match chosen {
            Some(term) => {
                if unique {
                    self.hits.insert(id, term.clone());
                }
                Found::One(term)
            }
            None => Found::Miss,
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

impl<S> fmt::Debug for Terms<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Terms")
            .field("term_table", &self.term_table)
            .field("taxonomy_table", &self.taxonomy_table)
            .finish_non_exhaustive()
    }
}
