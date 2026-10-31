//! One users-table row as the server sent it.

use std::fmt;

use cms_db::{Cell, ResultSet};

use crate::UserId;

const LOGIN: usize = 0;
const PASSWORD: usize = 1;
const NICENAME: usize = 2;
const EMAIL: usize = 3;
const URL: usize = 4;
const REGISTERED: usize = 5;
const ACTIVATION_KEY: usize = 6;
const STATUS: usize = 7;
const DISPLAY_NAME: usize = 8;
const CELLS: usize = 9;

/// A stored user row. Password, email and activation key are redacted in [`Debug`].
#[derive(Clone, PartialEq, Eq)]
pub struct User {
    id: UserId,
    cells: [Box<[u8]>; CELLS],
}

impl User {
    /// The row id.
    #[must_use]
    pub const fn id(&self) -> UserId {
        self.id
    }

    /// `user_login` bytes.
    #[must_use]
    pub fn login(&self) -> &[u8] {
        &self.cells[LOGIN]
    }

    /// `user_pass` bytes.
    #[must_use]
    pub fn password(&self) -> &[u8] {
        &self.cells[PASSWORD]
    }

    /// `user_nicename` bytes.
    #[must_use]
    pub fn nicename(&self) -> &[u8] {
        &self.cells[NICENAME]
    }

    /// `user_email` bytes.
    #[must_use]
    pub fn email(&self) -> &[u8] {
        &self.cells[EMAIL]
    }

    /// `user_url` bytes.
    #[must_use]
    pub fn url(&self) -> &[u8] {
        &self.cells[URL]
    }

    /// `user_registered` bytes.
    #[must_use]
    pub fn registered(&self) -> &[u8] {
        &self.cells[REGISTERED]
    }

    /// `user_activation_key` bytes.
    #[must_use]
    pub fn activation_key(&self) -> &[u8] {
        &self.cells[ACTIVATION_KEY]
    }

    /// `user_status` bytes.
    #[must_use]
    pub fn status(&self) -> &[u8] {
        &self.cells[STATUS]
    }

    /// `display_name` bytes.
    #[must_use]
    pub fn display_name(&self) -> &[u8] {
        &self.cells[DISPLAY_NAME]
    }
}

impl fmt::Debug for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("User")
            .field("id", &self.id)
            .field("login", &String::from_utf8_lossy(self.login()))
            .field("status", &String::from_utf8_lossy(self.status()))
            .finish_non_exhaustive()
    }
}

pub(crate) fn from_set(set: &ResultSet) -> Option<User> {
    let row = set.rows().first()?;
    let cells = row.cells();
    let mut id = None;
    let mut stored = empty_cells();
    for (column, cell) in set.columns().iter().zip(cells.iter()) {
        let bytes = cell_bytes(cell);
        if column.as_bytes() == b"ID" {
            id = UserId::from_bytes(&bytes).ok();
            continue;
        }
        if let Some(index) = column_index(column.as_bytes()) {
            stored[index] = bytes;
        }
    }
    Some(User {
        id: id?,
        cells: stored,
    })
}

fn empty_cells() -> [Box<[u8]>; CELLS] {
    [(); CELLS].map(|()| Box::from([]))
}

fn cell_bytes(cell: &Cell) -> Box<[u8]> {
    match cell {
        Cell::Null => Box::from([]),
        Cell::Bytes(bytes) => bytes.clone(),
    }
}

fn column_index(name: &[u8]) -> Option<usize> {
    match name {
        b"user_login" => Some(LOGIN),
        b"user_pass" => Some(PASSWORD),
        b"user_nicename" => Some(NICENAME),
        b"user_email" => Some(EMAIL),
        b"user_url" => Some(URL),
        b"user_registered" => Some(REGISTERED),
        b"user_activation_key" => Some(ACTIVATION_KEY),
        b"user_status" => Some(STATUS),
        b"display_name" => Some(DISPLAY_NAME),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{column_index, from_set};
    use cms_db::{Cell, Column, ResultSet, Row};

    fn named(pairs: &[(&str, Cell)]) -> ResultSet {
        let columns: Vec<Column> = pairs.iter().map(|(name, _)| Column::new(*name)).collect();
        let cells: Vec<Cell> = pairs.iter().map(|(_, cell)| cell.clone()).collect();
        ResultSet::new(columns, [Row::new(cells)])
    }

    #[test]
    fn a_row_maps_named_columns_and_redacts_debug() {
        let set = named(&[
            ("ID", Cell::bytes("7")),
            ("user_login", Cell::bytes("ada")),
            ("user_pass", Cell::bytes("$P$secret")),
            ("user_nicename", Cell::bytes("ada")),
            ("user_email", Cell::bytes("ada@example.test")),
            ("user_url", Cell::bytes("https://ada.test")),
            ("user_registered", Cell::bytes("2026-01-01 00:00:00")),
            ("user_activation_key", Cell::bytes("reset-token")),
            ("user_status", Cell::bytes("0")),
            ("display_name", Cell::bytes("Ada")),
            ("unknown", Cell::bytes("x")),
        ]);
        let user = from_set(&set);
        assert!(user.is_some());
        if let Some(user) = user {
            assert_eq!(user.id().as_u64(), 7);
            assert_eq!(user.login(), b"ada");
            assert_eq!(user.password(), b"$P$secret");
            assert_eq!(user.nicename(), b"ada");
            assert_eq!(user.email(), b"ada@example.test");
            assert_eq!(user.url(), b"https://ada.test");
            assert_eq!(user.registered(), b"2026-01-01 00:00:00");
            assert_eq!(user.activation_key(), b"reset-token");
            assert_eq!(user.status(), b"0");
            assert_eq!(user.display_name(), b"Ada");
            let shown = format!("{user:?}");
            assert!(shown.contains("User"));
            assert!(shown.contains("ada"));
            assert!(!shown.contains("$P$secret"));
            assert!(!shown.contains("ada@example.test"));
            assert!(!shown.contains("reset-token"));
        }
        assert!(from_set(&ResultSet::new(Vec::<Column>::new(), Vec::<Row>::new())).is_none());
        assert!(from_set(&named(&[("user_login", Cell::bytes("x"))])).is_none());
        assert!(column_index(b"nope").is_none());
    }
}
