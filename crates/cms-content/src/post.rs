//! One posts-table row as the server sent it.

use std::fmt;

use cms_db::{Cell, ResultSet};

use crate::PostId;

const AUTHOR: usize = 0;
const DATE: usize = 1;
const DATE_GMT: usize = 2;
const CONTENT: usize = 3;
const TITLE: usize = 4;
const EXCERPT: usize = 5;
const STATUS: usize = 6;
const COMMENT_STATUS: usize = 7;
const PING_STATUS: usize = 8;
const PASSWORD: usize = 9;
const NAME: usize = 10;
const TO_PING: usize = 11;
const PINGED: usize = 12;
const MODIFIED: usize = 13;
const MODIFIED_GMT: usize = 14;
const CONTENT_FILTERED: usize = 15;
const PARENT: usize = 16;
const GUID: usize = 17;
const MENU_ORDER: usize = 18;
const TYPE_NAME: usize = 19;
const MIME_TYPE: usize = 20;
const COMMENT_COUNT: usize = 21;
const CELLS: usize = 22;

/// A stored post row. Password is redacted in [`Debug`].
#[derive(Clone, PartialEq, Eq)]
pub struct Post {
    id: PostId,
    cells: [Box<[u8]>; CELLS],
}

impl Post {
    /// The row id.
    #[must_use]
    pub const fn id(&self) -> PostId {
        self.id
    }

    /// `post_author` bytes.
    #[must_use]
    pub fn author(&self) -> &[u8] {
        &self.cells[AUTHOR]
    }

    /// `post_date` bytes.
    #[must_use]
    pub fn date(&self) -> &[u8] {
        &self.cells[DATE]
    }

    /// `post_date_gmt` bytes.
    #[must_use]
    pub fn date_gmt(&self) -> &[u8] {
        &self.cells[DATE_GMT]
    }

    /// `post_content` bytes.
    #[must_use]
    pub fn content(&self) -> &[u8] {
        &self.cells[CONTENT]
    }

    /// `post_title` bytes.
    #[must_use]
    pub fn title(&self) -> &[u8] {
        &self.cells[TITLE]
    }

    /// `post_excerpt` bytes.
    #[must_use]
    pub fn excerpt(&self) -> &[u8] {
        &self.cells[EXCERPT]
    }

    /// `post_status` bytes.
    #[must_use]
    pub fn status(&self) -> &[u8] {
        &self.cells[STATUS]
    }

    /// `comment_status` bytes.
    #[must_use]
    pub fn comment_status(&self) -> &[u8] {
        &self.cells[COMMENT_STATUS]
    }

    /// `ping_status` bytes.
    #[must_use]
    pub fn ping_status(&self) -> &[u8] {
        &self.cells[PING_STATUS]
    }

    /// `post_password` bytes.
    #[must_use]
    pub fn password(&self) -> &[u8] {
        &self.cells[PASSWORD]
    }

    /// `post_name` bytes.
    #[must_use]
    pub fn name(&self) -> &[u8] {
        &self.cells[NAME]
    }

    /// `to_ping` bytes.
    #[must_use]
    pub fn to_ping(&self) -> &[u8] {
        &self.cells[TO_PING]
    }

    /// `pinged` bytes.
    #[must_use]
    pub fn pinged(&self) -> &[u8] {
        &self.cells[PINGED]
    }

    /// `post_modified` bytes.
    #[must_use]
    pub fn modified(&self) -> &[u8] {
        &self.cells[MODIFIED]
    }

    /// `post_modified_gmt` bytes.
    #[must_use]
    pub fn modified_gmt(&self) -> &[u8] {
        &self.cells[MODIFIED_GMT]
    }

    /// `post_content_filtered` bytes.
    #[must_use]
    pub fn content_filtered(&self) -> &[u8] {
        &self.cells[CONTENT_FILTERED]
    }

    /// `post_parent` bytes.
    #[must_use]
    pub fn parent(&self) -> &[u8] {
        &self.cells[PARENT]
    }

    /// `guid` bytes.
    #[must_use]
    pub fn guid(&self) -> &[u8] {
        &self.cells[GUID]
    }

    /// `menu_order` bytes.
    #[must_use]
    pub fn menu_order(&self) -> &[u8] {
        &self.cells[MENU_ORDER]
    }

    /// `post_type` bytes.
    #[must_use]
    pub fn type_name(&self) -> &[u8] {
        &self.cells[TYPE_NAME]
    }

    /// `post_mime_type` bytes.
    #[must_use]
    pub fn mime_type(&self) -> &[u8] {
        &self.cells[MIME_TYPE]
    }

    /// `comment_count` bytes.
    #[must_use]
    pub fn comment_count(&self) -> &[u8] {
        &self.cells[COMMENT_COUNT]
    }
}

impl fmt::Debug for Post {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Post")
            .field("id", &self.id)
            .field("status", &String::from_utf8_lossy(self.status()))
            .field("type_name", &String::from_utf8_lossy(self.type_name()))
            .finish_non_exhaustive()
    }
}

pub(crate) fn from_set(set: &ResultSet) -> Option<Post> {
    let row = set.rows().first()?;
    let cells = row.cells();
    let mut id = None;
    let mut stored = empty_cells();
    for (column, cell) in set.columns().iter().zip(cells.iter()) {
        let bytes = cell_bytes(cell);
        if column.as_bytes() == b"ID" {
            id = PostId::from_bytes(&bytes).ok();
            continue;
        }
        if let Some(index) = column_index(column.as_bytes()) {
            stored[index] = bytes;
        }
    }
    Some(Post {
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
        b"post_author" => Some(AUTHOR),
        b"post_date" => Some(DATE),
        b"post_date_gmt" => Some(DATE_GMT),
        b"post_content" => Some(CONTENT),
        b"post_title" => Some(TITLE),
        b"post_excerpt" => Some(EXCERPT),
        b"post_status" => Some(STATUS),
        b"comment_status" => Some(COMMENT_STATUS),
        b"ping_status" => Some(PING_STATUS),
        b"post_password" => Some(PASSWORD),
        b"post_name" => Some(NAME),
        b"to_ping" => Some(TO_PING),
        b"pinged" => Some(PINGED),
        b"post_modified" => Some(MODIFIED),
        b"post_modified_gmt" => Some(MODIFIED_GMT),
        b"post_content_filtered" => Some(CONTENT_FILTERED),
        b"post_parent" => Some(PARENT),
        b"guid" => Some(GUID),
        b"menu_order" => Some(MENU_ORDER),
        b"post_type" => Some(TYPE_NAME),
        b"post_mime_type" => Some(MIME_TYPE),
        b"comment_count" => Some(COMMENT_COUNT),
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
            ("post_author", Cell::bytes("1")),
            ("post_date", Cell::bytes("2026-01-01 00:00:00")),
            ("post_date_gmt", Cell::bytes("2026-01-01 00:00:00")),
            ("post_content", Cell::bytes("body")),
            ("post_title", Cell::bytes("Hello")),
            ("post_excerpt", Cell::bytes("ex")),
            ("post_status", Cell::bytes("publish")),
            ("comment_status", Cell::bytes("open")),
            ("ping_status", Cell::bytes("closed")),
            ("post_password", Cell::bytes("s3cret")),
            ("post_name", Cell::bytes("hello")),
            ("to_ping", Cell::bytes("a")),
            ("pinged", Cell::bytes("b")),
            ("post_modified", Cell::bytes("2026-01-02 00:00:00")),
            ("post_modified_gmt", Cell::bytes("2026-01-02 00:00:00")),
            ("post_content_filtered", Cell::bytes("filt")),
            ("post_parent", Cell::bytes("0")),
            ("guid", Cell::bytes("http://example.test/?p=7")),
            ("menu_order", Cell::bytes("3")),
            ("post_type", Cell::bytes("post")),
            ("post_mime_type", Cell::bytes("")),
            ("comment_count", Cell::bytes("2")),
            ("unknown", Cell::bytes("x")),
        ]);
        let post = from_set(&set);
        assert!(post.is_some());
        if let Some(post) = post {
            assert_eq!(post.id().as_u64(), 7);
            assert_eq!(post.author(), b"1");
            assert_eq!(post.date(), b"2026-01-01 00:00:00");
            assert_eq!(post.date_gmt(), b"2026-01-01 00:00:00");
            assert_eq!(post.content(), b"body");
            assert_eq!(post.title(), b"Hello");
            assert_eq!(post.excerpt(), b"ex");
            assert_eq!(post.status(), b"publish");
            assert_eq!(post.comment_status(), b"open");
            assert_eq!(post.ping_status(), b"closed");
            assert_eq!(post.password(), b"s3cret");
            assert_eq!(post.name(), b"hello");
            assert_eq!(post.to_ping(), b"a");
            assert_eq!(post.pinged(), b"b");
            assert_eq!(post.modified(), b"2026-01-02 00:00:00");
            assert_eq!(post.modified_gmt(), b"2026-01-02 00:00:00");
            assert_eq!(post.content_filtered(), b"filt");
            assert_eq!(post.parent(), b"0");
            assert_eq!(post.guid(), b"http://example.test/?p=7");
            assert_eq!(post.menu_order(), b"3");
            assert_eq!(post.type_name(), b"post");
            assert_eq!(post.mime_type(), b"");
            assert_eq!(post.comment_count(), b"2");
            let shown = format!("{post:?}");
            assert!(shown.contains("Post"));
            assert!(!shown.contains("s3cret"));
        }
        assert!(from_set(&ResultSet::new(Vec::<Column>::new(), Vec::<Row>::new())).is_none());
        assert!(from_set(&named(&[("post_title", Cell::bytes("x"))])).is_none());
        assert!(column_index(b"nope").is_none());
    }
}
