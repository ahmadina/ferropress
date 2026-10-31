//! Request-local autoload bag, per-name hits, and remembered misses.

use std::collections::{HashMap, HashSet};
use std::fmt;

use cms_db::{Cell, Reply, ResultSet, Sql, Store, TableName, TablePrefix, TableStem};

use crate::autoload::{Autoload, is_auto_family, is_loaded_token};
use crate::error::Error;
use crate::name::OptionName;
use crate::sql::{
    delete_row, insert_row, select_all, select_autoload, select_autoloaded, select_value,
    update_row,
};
use crate::write::Write;

/// Options table stem from `contracts/inventory/tables.jsonl`.
const STEM: &[u8] = b"options";

/// One site's option rows, loaded through [`Store`].
///
/// Request-affine: not assumed [`Send`]. Contracts: `contracts/options/load.md`,
/// `contracts/options/write.md`.
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

    /// Insert `name` unless it already exists.
    ///
    /// Contract: `contracts/options/write.md`.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn add(
        &mut self,
        name: &OptionName,
        value: &[u8],
        autoload: Autoload,
    ) -> Result<Write, Error> {
        if name.is_protected() {
            return Ok(Write::Protected);
        }
        self.load()?;
        if self.raw_get(name)?.is_some() {
            return Ok(Write::Unchanged);
        }
        if !self.exec(&insert_row(&self.table, name, value, autoload))? {
            return Ok(Write::Unchanged);
        }
        self.remember(name, value, Some(autoload))?;
        Ok(Write::Changed)
    }

    /// Replace the stored bytes, or [`add`](Self::add) when the name is missing.
    ///
    /// Contract: `contracts/options/write.md`.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn update(
        &mut self,
        name: &OptionName,
        value: &[u8],
        autoload: Option<Autoload>,
    ) -> Result<Write, Error> {
        if name.is_protected() {
            return Ok(Write::Protected);
        }
        self.load()?;
        match self.raw_get(name)? {
            None => self.add(name, value, autoload.unwrap_or(Autoload::Auto)),
            Some(old) if old.as_ref() == value => Ok(Write::Unchanged),
            Some(_) => {
                let extra = match autoload {
                    Some(autoload) => Some(autoload),
                    None => self.reevaluate_auto(name)?,
                };
                if !self.exec(&update_row(&self.table, name, value, extra))? {
                    return Ok(Write::Unchanged);
                }
                self.remember(name, value, extra)?;
                Ok(Write::Changed)
            }
        }
    }

    /// Remove `name` if the row exists.
    ///
    /// Contract: `contracts/options/write.md`.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn delete(&mut self, name: &OptionName) -> Result<Write, Error> {
        if name.is_protected() {
            return Ok(Write::Protected);
        }
        let Some(token) = self.autoload_token(name)? else {
            return Ok(Write::Unchanged);
        };
        if !self.exec(&delete_row(&self.table, name))? {
            return Ok(Write::Unchanged);
        }
        self.forget(name, is_loaded_token(&token));
        Ok(Write::Changed)
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

    fn exec(&mut self, sql: &Sql) -> Result<bool, Error> {
        match self.store.query(sql) {
            Ok(Reply::Done { affected, .. }) => Ok(affected > 0),
            Ok(Reply::Rows(_)) => Ok(true),
            Ok(Reply::Failed(_) | _) => Ok(false),
            Err(_) => Err(Error::Unavailable),
        }
    }

    fn autoload_token(&mut self, name: &OptionName) -> Result<Option<Box<[u8]>>, Error> {
        match self.query(&select_autoload(&self.table, name))? {
            RowsOrFailed::Rows(set) => Ok(first_value(&set)),
            RowsOrFailed::Failed => Ok(None),
        }
    }

    fn reevaluate_auto(&mut self, name: &OptionName) -> Result<Option<Autoload>, Error> {
        let Some(raw) = self.autoload_token(name)? else {
            return Ok(None);
        };
        if is_auto_family(&raw) && raw.as_ref() != Autoload::Auto.as_bytes() {
            Ok(Some(Autoload::Auto))
        } else {
            Ok(None)
        }
    }

    fn remember(
        &mut self,
        name: &OptionName,
        value: &[u8],
        autoload: Option<Autoload>,
    ) -> Result<(), Error> {
        self.notoptions.remove(name);
        let stored: Box<[u8]> = value.into();
        match autoload {
            Some(autoload) if autoload.is_loaded() => {
                self.load()?;
                if let Some(all) = &mut self.alloptions {
                    all.insert(name.clone(), stored);
                }
                self.singles.remove(name);
            }
            Some(_) => {
                self.load()?;
                if let Some(all) = &mut self.alloptions {
                    all.remove(name);
                }
                self.singles.insert(name.clone(), stored);
            }
            None => {
                if let Some(all) = &mut self.alloptions
                    && all.contains_key(name)
                {
                    all.insert(name.clone(), stored);
                } else {
                    self.singles.insert(name.clone(), stored);
                }
            }
        }
        Ok(())
    }

    fn forget(&mut self, name: &OptionName, was_loaded: bool) {
        if was_loaded {
            if let Some(all) = &mut self.alloptions {
                all.remove(name);
            }
        } else {
            self.singles.remove(name);
        }
        self.notoptions.insert(name.clone());
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
