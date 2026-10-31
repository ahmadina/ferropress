//! Request-local autoload bag, per-name hits, and remembered misses.

use std::collections::{HashMap, HashSet};
use std::fmt;

use cms_db::{Cell, Reply, ResultSet, Sql, Store, TableName, TablePrefix, TableStem};

use crate::error::Error;
use crate::name::OptionName;
use crate::sql::{select_all, select_autoloaded, select_value};

/// Options table stem from `contracts/inventory/tables.jsonl`.
const STEM: &[u8] = b"options";

/// One site's option rows, loaded through [`Store`].
///
/// Request-affine: not assumed [`Send`]. Contract: `contracts/options/load.md`.
pub struct Options<S> {
    store: S,
    table: TableName,
    alloptions: Option<HashMap<OptionName, Box<[u8]>>>,
    notoptions: HashSet<OptionName>,
    singles: HashMap<OptionName, Box<[u8]>>,
}

impl<S: Store> Options<S> {
    /// A bag that will read `{prefix}options`.
    ///
    /// # Errors
    ///
    /// [`Error::Table`] when the prefix contains a NUL.
    pub fn new(store: S, prefix: &TablePrefix) -> Result<Self, Error> {
        let stem = TableStem::new(STEM)?;
        Ok(Self {
            store,
            table: prefix.table(&stem),
            alloptions: None,
            notoptions: HashSet::new(),
            singles: HashMap::new(),
        })
    }

    /// Load autoloaded rows (or every row if that set is empty).
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn load(&mut self) -> Result<(), Error> {
        if self.alloptions.is_some() {
            return Ok(());
        }
        let rows = match self.query(&select_autoloaded(&self.table))? {
            RowsOrFailed::Rows(set) if !set.rows().is_empty() => set,
            RowsOrFailed::Rows(_) | RowsOrFailed::Failed => {
                match self.query(&select_all(&self.table))? {
                    RowsOrFailed::Rows(set) => set,
                    RowsOrFailed::Failed => ResultSet::new(Vec::new(), Vec::new()),
                }
            }
        };
        self.alloptions = Some(map_name_value(&rows));
        Ok(())
    }

    /// Stored bytes for `name`, or `None` when the row does not exist.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn get(&mut self, name: &OptionName) -> Result<Option<Box<[u8]>>, Error> {
        self.load()?;
        let raw = self.raw_get(name)?;
        let Some(bytes) = raw else {
            return Ok(None);
        };
        if name.is_home() && bytes.is_empty() {
            return self.get(&OptionName::siteurl());
        }
        Ok(Some(finish(name, bytes)))
    }

    fn raw_get(&mut self, name: &OptionName) -> Result<Option<Box<[u8]>>, Error> {
        if let Some(all) = &self.alloptions
            && let Some(value) = all.get(name)
        {
            return Ok(Some(value.clone()));
        }
        if self.notoptions.contains(name) {
            return Ok(None);
        }
        if let Some(value) = self.singles.get(name) {
            return Ok(Some(value.clone()));
        }
        match self.query(&select_value(&self.table, name))? {
            RowsOrFailed::Rows(set) => {
                if let Some(value) = first_value(&set) {
                    self.singles.insert(name.clone(), value.clone());
                    Ok(Some(value))
                } else {
                    self.notoptions.insert(name.clone());
                    Ok(None)
                }
            }
            RowsOrFailed::Failed => {
                self.notoptions.insert(name.clone());
                Ok(None)
            }
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

fn map_name_value(set: &ResultSet) -> HashMap<OptionName, Box<[u8]>> {
    let mut map = HashMap::with_capacity(set.rows().len());
    for row in set.rows() {
        let cells = row.cells();
        let Some(name_cell) = cells.first() else {
            continue;
        };
        let value = cells.get(1).map_or_else(|| Box::from([]), cell_bytes);
        let Cell::Bytes(name) = name_cell else {
            continue;
        };
        if let Ok(option) = OptionName::new(name.as_ref()) {
            map.insert(option, value);
        }
    }
    map
}

fn first_value(set: &ResultSet) -> Option<Box<[u8]>> {
    let row = set.rows().first()?;
    Some(cell_bytes(row.cells().first()?))
}

fn cell_bytes(cell: &Cell) -> Box<[u8]> {
    match cell {
        Cell::Null => Box::from([]),
        Cell::Bytes(bytes) => bytes.clone(),
    }
}

fn finish(name: &OptionName, bytes: Box<[u8]>) -> Box<[u8]> {
    if name.strips_trailing_slash() {
        untrail(&bytes)
    } else {
        bytes
    }
}

fn untrail(bytes: &[u8]) -> Box<[u8]> {
    let mut end = bytes.len();
    while end > 0 && (bytes[end - 1] == b'/' || bytes[end - 1] == b'\\') {
        end -= 1;
    }
    bytes[..end].into()
}

impl<S> fmt::Debug for Options<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Options")
            .field("table", &self.table)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::untrail;

    #[test]
    fn trailing_slashes_and_backslashes_are_stripped() {
        assert_eq!(
            untrail(b"http://example.test/").as_ref(),
            b"http://example.test"
        );
        assert_eq!(untrail(b"a\\\\").as_ref(), b"a");
        assert_eq!(untrail(b"plain").as_ref(), b"plain");
        assert_eq!(untrail(b"").as_ref(), b"");
    }
}
