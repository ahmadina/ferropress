//! Request-local term hits on a [`cms_db::Store`].

use std::collections::HashMap;
use std::fmt;

use cms_db::{Cell, Reply, ResultSet, Sql, Store, TableName, TablePrefix, TableStem};

use crate::draft::Draft;
use crate::error::Error;
use crate::found::Found;
use crate::id::{TermId, TermTaxonomyId};
use crate::sql::{
    count_taxonomies, delete_taxonomy, delete_term, insert_taxonomy, insert_term, select_rows,
    update_taxonomy, update_term,
};
use crate::term::{Term, from_set};
use crate::write::Write;

/// Terms table stem from `contracts/inventory/tables.jsonl`.
const TERMS: &[u8] = b"terms";
/// Term taxonomy table stem from `contracts/inventory/tables.jsonl`.
const TAXONOMY: &[u8] = b"term_taxonomy";

/// One site's term rows, loaded through [`Store`].
///
/// Request-affine: not assumed [`Send`]. Contracts: `contracts/taxonomy/get.md`,
/// `contracts/taxonomy/write.md`.
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

    /// Insert a terms row, then its taxonomy row. Ids come from `insert_id`.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn insert(&mut self, draft: &Draft) -> Result<Write, Error> {
        let term_id = match self.exec(&insert_term(&self.term_table, draft))? {
            Reply::Done { insert_id, .. } => match TermId::new(insert_id) {
                Ok(id) => id,
                Err(_) => return Ok(Write::Unchanged),
            },
            _ => return Ok(Write::Unchanged),
        };
        match self.exec(&insert_taxonomy(&self.taxonomy_table, term_id, draft))? {
            Reply::Done { insert_id, .. } => match TermTaxonomyId::new(insert_id) {
                Ok(taxonomy_id) => Ok(Write::Inserted {
                    term_id,
                    taxonomy_id,
                }),
                Err(_) => Ok(Write::Unchanged),
            },
            _ => Ok(Write::Unchanged),
        }
    }

    /// Rewrite the terms and taxonomy cells for `id` / `taxonomy_id`.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn update(
        &mut self,
        id: TermId,
        taxonomy_id: TermTaxonomyId,
        draft: &Draft,
    ) -> Result<Write, Error> {
        let terms = self.exec(&update_term(&self.term_table, id, draft))?;
        self.hits.remove(&id);
        let taxonomy = self.exec(&update_taxonomy(
            &self.taxonomy_table,
            id,
            taxonomy_id,
            draft,
        ))?;
        if affected(&terms) > 0 || affected(&taxonomy) > 0 {
            Ok(Write::Replaced)
        } else {
            Ok(Write::Unchanged)
        }
    }

    /// Remove the taxonomy row, and the terms row when nothing else uses `id`.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn delete(&mut self, id: TermId, taxonomy_id: TermTaxonomyId) -> Result<Write, Error> {
        let taxonomy = self.exec(&delete_taxonomy(&self.taxonomy_table, taxonomy_id))?;
        self.hits.remove(&id);
        let leftover = match self.query(&count_taxonomies(&self.taxonomy_table, id))? {
            RowsOrFailed::Rows(set) => leftover_count(&set),
            RowsOrFailed::Failed => 0,
        };
        let terms = if leftover == 0 {
            affected(&self.exec(&delete_term(&self.term_table, id))?)
        } else {
            0
        };
        if affected(&taxonomy) > 0 || terms > 0 {
            Ok(Write::Removed)
        } else {
            Ok(Write::Unchanged)
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

    fn exec(&mut self, sql: &Sql) -> Result<Reply, Error> {
        match self.store.query(sql) {
            Ok(reply) => Ok(reply),
            Err(_) => Err(Error::Unavailable),
        }
    }
}

fn affected(reply: &Reply) -> u64 {
    match reply {
        Reply::Done { affected, .. } => *affected,
        _ => 0,
    }
}

fn leftover_count(set: &ResultSet) -> u64 {
    let Some(row) = set.rows().first() else {
        return 0;
    };
    let Some(cell) = row.cells().first() else {
        return 0;
    };
    match cell {
        Cell::Null => 0,
        Cell::Bytes(bytes) => {
            let Ok(text) = std::str::from_utf8(bytes) else {
                return 0;
            };
            let Ok(count) = text.parse::<u64>() else {
                return 0;
            };
            count
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
