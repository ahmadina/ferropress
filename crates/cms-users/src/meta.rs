//! Request-local usermeta bags on a [`cms_db::Store`].

use std::collections::HashMap;
use std::fmt;

use cms_db::{Cell, Reply, ResultSet, Row, Sql, Store, TableName, TablePrefix, TableStem};

use crate::error::Error;
use crate::id::UserId;
use crate::key::MetaKey;
use crate::sql::select_meta;

/// Usermeta table stem from `contracts/inventory/tables.jsonl`.
const STEM: &[u8] = b"usermeta";

type Values = Vec<Box<[u8]>>;
type KeyBag = HashMap<MetaKey, Values>;

/// One network's user-meta rows, loaded through [`Store`].
///
/// Request-affine: not assumed [`Send`]. Contract: `contracts/users/meta.md`.
pub struct UserMeta<S> {
    store: S,
    table: TableName,
    bags: HashMap<UserId, KeyBag>,
}

impl<S: Store> UserMeta<S> {
    /// A bag that will read `{prefix}usermeta`.
    ///
    /// # Errors
    ///
    /// [`Error::Table`] when the prefix contains a NUL.
    pub fn new(store: S, prefix: &TablePrefix) -> Result<Self, Error> {
        let stem = TableStem::new(STEM)?;
        Ok(Self {
            store,
            table: prefix.table(&stem),
            bags: HashMap::new(),
        })
    }

    /// Every stored value for `key` on `id`, in `umeta_id` order.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn get(&mut self, id: UserId, key: &MetaKey) -> Result<Option<&[Box<[u8]>]>, Error> {
        self.load(id)?;
        Ok(self
            .bags
            .get(&id)
            .and_then(|bag| bag.get(key))
            .map(Vec::as_slice))
    }

    /// The first stored value for `key` on `id`.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the store cannot talk to a server.
    pub fn first(&mut self, id: UserId, key: &MetaKey) -> Result<Option<&[u8]>, Error> {
        Ok(self
            .get(id, key)?
            .and_then(|values| values.first().map(AsRef::as_ref)))
    }

    fn load(&mut self, id: UserId) -> Result<(), Error> {
        if self.bags.contains_key(&id) {
            return Ok(());
        }
        let bag = match self.query(&select_meta(&self.table, id))? {
            RowsOrFailed::Rows(set) => bag_from_set(&set, id),
            RowsOrFailed::Failed => KeyBag::new(),
        };
        self.bags.insert(id, bag);
        Ok(())
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

impl<S> fmt::Debug for UserMeta<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserMeta")
            .field("table", &self.table)
            .finish_non_exhaustive()
    }
}

fn bag_from_set(set: &ResultSet, expected: UserId) -> KeyBag {
    let user_col = column_index(set, b"user_id");
    let key_col = column_index(set, b"meta_key");
    let value_col = column_index(set, b"meta_value");
    let mut bag = KeyBag::new();
    for row in set.rows() {
        if cell_id(row, user_col) != Some(expected) {
            continue;
        }
        let Ok(key) = MetaKey::new(cell_bytes(row, key_col)) else {
            continue;
        };
        bag.entry(key).or_default().push(cell_bytes(row, value_col));
    }
    bag
}

fn column_index(set: &ResultSet, name: &[u8]) -> Option<usize> {
    set.columns()
        .iter()
        .position(|column| column.as_bytes() == name)
}

fn cell_bytes(row: &Row, index: Option<usize>) -> Box<[u8]> {
    match index.and_then(|index| row.cells().get(index)) {
        Some(Cell::Bytes(bytes)) => bytes.clone(),
        Some(Cell::Null) | None => Box::from([]),
    }
}

fn cell_id(row: &Row, index: Option<usize>) -> Option<UserId> {
    UserId::from_bytes(&cell_bytes(row, index)).ok()
}

#[cfg(test)]
mod tests {
    use super::bag_from_set;
    use cms_db::{Cell, Column, ResultSet, Row};

    use crate::UserId;
    use crate::key::MetaKey;

    fn set(rows: Vec<Row>) -> ResultSet {
        ResultSet::new(
            [
                Column::new("user_id"),
                Column::new("meta_key"),
                Column::new("meta_value"),
            ],
            rows,
        )
    }

    #[test]
    fn rows_group_by_key_and_skip_junk() {
        let id = UserId::new(12);
        assert!(id.is_ok());
        if let Ok(id) = id {
            let mapped = bag_from_set(
                &set(vec![
                    Row::new([
                        Cell::bytes("12"),
                        Cell::bytes("nickname"),
                        Cell::bytes("Ada"),
                    ]),
                    Row::new([
                        Cell::bytes("12"),
                        Cell::bytes("nickname"),
                        Cell::bytes("Ada Two"),
                    ]),
                    Row::new([Cell::bytes("13"), Cell::bytes("nickname"), Cell::bytes("x")]),
                    Row::new([Cell::bytes("12"), Cell::Null, Cell::bytes("skip")]),
                    Row::new([Cell::bytes("12"), Cell::bytes(""), Cell::bytes("skip")]),
                    Row::new([Cell::bytes("12"), Cell::bytes("session"), Cell::Null]),
                ]),
                id,
            );
            let nick = MetaKey::new("nickname");
            let session = MetaKey::new("session");
            assert!(nick.is_ok() && session.is_ok());
            if let (Ok(nick), Ok(session)) = (nick, session) {
                let values = mapped.get(&nick);
                assert!(values.is_some_and(|values| {
                    values.len() == 2
                        && values[0].as_ref() == b"Ada"
                        && values[1].as_ref() == b"Ada Two"
                }));
                assert!(
                    mapped
                        .get(&session)
                        .is_some_and(|values| values[0].is_empty())
                );
            }
            assert!(
                bag_from_set(
                    &ResultSet::new([Column::new("other")], [Row::new([Cell::bytes("12")])]),
                    id
                )
                .is_empty()
            );
        }
    }
}
