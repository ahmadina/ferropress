//! One comments-table row as the server sent it.

use std::fmt;

use cms_db::{Cell, ResultSet};

use crate::CommentId;

const POST_ID: usize = 0;
const AUTHOR: usize = 1;
const EMAIL: usize = 2;
const URL: usize = 3;
const IP: usize = 4;
const DATE: usize = 5;
const DATE_GMT: usize = 6;
const CONTENT: usize = 7;
const KARMA: usize = 8;
const APPROVED: usize = 9;
const AGENT: usize = 10;
const TYPE_NAME: usize = 11;
const PARENT: usize = 12;
const USER_ID: usize = 13;
const CELLS: usize = 14;

/// A stored comment row. Author email is redacted in [`Debug`].
#[derive(Clone, PartialEq, Eq)]
pub struct Comment {
    id: CommentId,
    cells: [Box<[u8]>; CELLS],
}

impl Comment {
    /// The row id.
    #[must_use]
    pub const fn id(&self) -> CommentId {
        self.id
    }

    /// `comment_post_ID` bytes.
    #[must_use]
    pub fn post_id(&self) -> &[u8] {
        &self.cells[POST_ID]
    }

    /// `comment_author` bytes.
    #[must_use]
    pub fn author(&self) -> &[u8] {
        &self.cells[AUTHOR]
    }

    /// `comment_author_email` bytes.
    #[must_use]
    pub fn author_email(&self) -> &[u8] {
        &self.cells[EMAIL]
    }

    /// `comment_author_url` bytes.
    #[must_use]
    pub fn author_url(&self) -> &[u8] {
        &self.cells[URL]
    }

    /// `comment_author_IP` bytes.
    #[must_use]
    pub fn author_ip(&self) -> &[u8] {
        &self.cells[IP]
    }

    /// `comment_date` bytes.
    #[must_use]
    pub fn date(&self) -> &[u8] {
        &self.cells[DATE]
    }

    /// `comment_date_gmt` bytes.
    #[must_use]
    pub fn date_gmt(&self) -> &[u8] {
        &self.cells[DATE_GMT]
    }

    /// `comment_content` bytes.
    #[must_use]
    pub fn content(&self) -> &[u8] {
        &self.cells[CONTENT]
    }

    /// `comment_karma` bytes.
    #[must_use]
    pub fn karma(&self) -> &[u8] {
        &self.cells[KARMA]
    }

    /// `comment_approved` bytes.
    #[must_use]
    pub fn approved(&self) -> &[u8] {
        &self.cells[APPROVED]
    }

    /// `comment_agent` bytes.
    #[must_use]
    pub fn agent(&self) -> &[u8] {
        &self.cells[AGENT]
    }

    /// `comment_type` bytes.
    #[must_use]
    pub fn type_name(&self) -> &[u8] {
        &self.cells[TYPE_NAME]
    }

    /// `comment_parent` bytes.
    #[must_use]
    pub fn parent(&self) -> &[u8] {
        &self.cells[PARENT]
    }

    /// `user_id` bytes.
    #[must_use]
    pub fn user_id(&self) -> &[u8] {
        &self.cells[USER_ID]
    }
}

impl fmt::Debug for Comment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Comment")
            .field("id", &self.id)
            .field("approved", &String::from_utf8_lossy(self.approved()))
            .field("type_name", &String::from_utf8_lossy(self.type_name()))
            .finish_non_exhaustive()
    }
}

pub(crate) fn from_set(set: &ResultSet) -> Option<Comment> {
    let row = set.rows().first()?;
    let cells = row.cells();
    let mut id = None;
    let mut stored = empty_cells();
    for (column, cell) in set.columns().iter().zip(cells.iter()) {
        let bytes = cell_bytes(cell);
        if column.as_bytes() == b"comment_ID" {
            id = CommentId::from_bytes(&bytes).ok();
            continue;
        }
        if let Some(index) = column_index(column.as_bytes()) {
            stored[index] = bytes;
        }
    }
    Some(Comment {
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
        b"comment_post_ID" => Some(POST_ID),
        b"comment_author" => Some(AUTHOR),
        b"comment_author_email" => Some(EMAIL),
        b"comment_author_url" => Some(URL),
        b"comment_author_IP" => Some(IP),
        b"comment_date" => Some(DATE),
        b"comment_date_gmt" => Some(DATE_GMT),
        b"comment_content" => Some(CONTENT),
        b"comment_karma" => Some(KARMA),
        b"comment_approved" => Some(APPROVED),
        b"comment_agent" => Some(AGENT),
        b"comment_type" => Some(TYPE_NAME),
        b"comment_parent" => Some(PARENT),
        b"user_id" => Some(USER_ID),
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
            ("comment_ID", Cell::bytes("7")),
            ("comment_post_ID", Cell::bytes("3")),
            ("comment_author", Cell::bytes("Ada")),
            ("comment_author_email", Cell::bytes("ada@example.test")),
            ("comment_author_url", Cell::bytes("https://ada.test")),
            ("comment_author_IP", Cell::bytes("203.0.113.4")),
            ("comment_date", Cell::bytes("2026-01-01 00:00:00")),
            ("comment_date_gmt", Cell::bytes("2026-01-01 00:00:00")),
            ("comment_content", Cell::bytes("hello")),
            ("comment_karma", Cell::bytes("0")),
            ("comment_approved", Cell::bytes("1")),
            ("comment_agent", Cell::bytes("agent")),
            ("comment_type", Cell::bytes("comment")),
            ("comment_parent", Cell::bytes("0")),
            ("user_id", Cell::bytes("2")),
            ("unknown", Cell::bytes("x")),
        ]);
        let comment = from_set(&set);
        assert!(comment.is_some());
        if let Some(comment) = comment {
            assert_eq!(comment.id().as_u64(), 7);
            assert_eq!(comment.post_id(), b"3");
            assert_eq!(comment.author(), b"Ada");
            assert_eq!(comment.author_email(), b"ada@example.test");
            assert_eq!(comment.author_url(), b"https://ada.test");
            assert_eq!(comment.author_ip(), b"203.0.113.4");
            assert_eq!(comment.date(), b"2026-01-01 00:00:00");
            assert_eq!(comment.date_gmt(), b"2026-01-01 00:00:00");
            assert_eq!(comment.content(), b"hello");
            assert_eq!(comment.karma(), b"0");
            assert_eq!(comment.approved(), b"1");
            assert_eq!(comment.agent(), b"agent");
            assert_eq!(comment.type_name(), b"comment");
            assert_eq!(comment.parent(), b"0");
            assert_eq!(comment.user_id(), b"2");
            let shown = format!("{comment:?}");
            assert!(shown.contains("Comment"));
            assert!(!shown.contains("ada@example.test"));
        }
        assert!(from_set(&ResultSet::new(Vec::<Column>::new(), Vec::<Row>::new())).is_none());
        assert!(from_set(&named(&[("comment_content", Cell::bytes("x"))])).is_none());
        assert!(column_index(b"nope").is_none());
    }
}
