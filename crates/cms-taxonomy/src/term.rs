//! One joined `terms` / `term_taxonomy` row as the server sent it.

use std::fmt;

use cms_db::{Cell, ResultSet, Row};

use crate::TermId;

const NAME: usize = 0;
const SLUG: usize = 1;
const TERM_GROUP: usize = 2;
const TAXONOMY_ID: usize = 3;
const TAXONOMY: usize = 4;
const DESCRIPTION: usize = 5;
const PARENT: usize = 6;
const COUNT: usize = 7;
const CELLS: usize = 8;

/// A stored term row.
#[derive(Clone, PartialEq, Eq)]
pub struct Term {
    id: TermId,
    cells: [Box<[u8]>; CELLS],
}

impl Term {
    /// The row id.
    #[must_use]
    pub const fn id(&self) -> TermId {
        self.id
    }

    /// `name` bytes.
    #[must_use]
    pub fn name(&self) -> &[u8] {
        &self.cells[NAME]
    }

    /// `slug` bytes.
    #[must_use]
    pub fn slug(&self) -> &[u8] {
        &self.cells[SLUG]
    }

    /// `term_group` bytes.
    #[must_use]
    pub fn term_group(&self) -> &[u8] {
        &self.cells[TERM_GROUP]
    }

    /// `term_taxonomy_id` bytes.
    #[must_use]
    pub fn term_taxonomy_id(&self) -> &[u8] {
        &self.cells[TAXONOMY_ID]
    }

    /// `taxonomy` bytes.
    #[must_use]
    pub fn taxonomy(&self) -> &[u8] {
        &self.cells[TAXONOMY]
    }

    /// `description` bytes.
    #[must_use]
    pub fn description(&self) -> &[u8] {
        &self.cells[DESCRIPTION]
    }

    /// `parent` bytes.
    #[must_use]
    pub fn parent(&self) -> &[u8] {
        &self.cells[PARENT]
    }

    /// `count` bytes.
    #[must_use]
    pub fn count(&self) -> &[u8] {
        &self.cells[COUNT]
    }
}

impl fmt::Debug for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Term")
            .field("id", &self.id)
            .field("taxonomy", &String::from_utf8_lossy(self.taxonomy()))
            .finish_non_exhaustive()
    }
}

pub(crate) fn from_set(set: &ResultSet) -> Vec<Term> {
    set.rows()
        .iter()
        .filter_map(|row| from_row(set.columns(), row))
        .collect()
}

fn from_row(columns: &[cms_db::Column], row: &Row) -> Option<Term> {
    let mut id = None;
    let mut stored = empty_cells();
    for (column, cell) in columns.iter().zip(row.cells().iter()) {
        let bytes = cell_bytes(cell);
        if column.as_bytes() == b"term_id" {
            id = TermId::from_bytes(&bytes).ok();
            continue;
        }
        if let Some(index) = column_index(column.as_bytes()) {
            stored[index] = bytes;
        }
    }
    Some(Term {
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
        b"name" => Some(NAME),
        b"slug" => Some(SLUG),
        b"term_group" => Some(TERM_GROUP),
        b"term_taxonomy_id" => Some(TAXONOMY_ID),
        b"taxonomy" => Some(TAXONOMY),
        b"description" => Some(DESCRIPTION),
        b"parent" => Some(PARENT),
        b"count" => Some(COUNT),
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
    fn a_row_maps_named_columns() {
        let set = named(&[
            ("term_id", Cell::bytes("7")),
            ("name", Cell::bytes("News")),
            ("slug", Cell::bytes("news")),
            ("term_group", Cell::bytes("0")),
            ("term_taxonomy_id", Cell::bytes("9")),
            ("taxonomy", Cell::bytes("category")),
            ("description", Cell::bytes("desc")),
            ("parent", Cell::bytes("1")),
            ("count", Cell::bytes("4")),
            ("unknown", Cell::bytes("x")),
        ]);
        let terms = from_set(&set);
        assert_eq!(terms.len(), 1);
        if let Some(term) = terms.first() {
            assert_eq!(term.id().as_u64(), 7);
            assert_eq!(term.name(), b"News");
            assert_eq!(term.slug(), b"news");
            assert_eq!(term.term_group(), b"0");
            assert_eq!(term.term_taxonomy_id(), b"9");
            assert_eq!(term.taxonomy(), b"category");
            assert_eq!(term.description(), b"desc");
            assert_eq!(term.parent(), b"1");
            assert_eq!(term.count(), b"4");
            let shown = format!("{term:?}");
            assert!(shown.contains("Term"));
            assert!(shown.contains("category"));
        }
        assert!(from_set(&ResultSet::new(Vec::<Column>::new(), Vec::<Row>::new())).is_empty());
        assert!(from_set(&named(&[("name", Cell::bytes("x"))])).is_empty());
        assert!(from_set(&named(&[("term_id", Cell::bytes("0"))])).is_empty());
        assert!(column_index(b"nope").is_none());
    }

    #[test]
    fn null_and_duplicate_term_id_columns() {
        let set = ResultSet::new(
            [
                Column::new("term_id"),
                Column::new("name"),
                Column::new("term_id"),
                Column::new("taxonomy"),
            ],
            [Row::new([
                Cell::bytes("3"),
                Cell::Null,
                Cell::bytes("3"),
                Cell::Null,
            ])],
        );
        let terms = from_set(&set);
        assert_eq!(terms.len(), 1);
        if let Some(term) = terms.first() {
            assert_eq!(term.id().as_u64(), 3);
            assert!(term.name().is_empty());
            assert!(term.taxonomy().is_empty());
        }
    }
}
