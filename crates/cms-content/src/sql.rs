//! Statements the reference issues for post reads and writes.

use cms_db::{Sql, TableName};

use crate::PostId;
use crate::draft::{Draft, Field};
use crate::quote::quote;

/// One row: `SELECT * … WHERE ID = {id} LIMIT 1`.
#[must_use]
pub(crate) fn select_row(table: &TableName, id: PostId) -> Sql {
    let mut sql = Vec::from(b"SELECT * FROM ");
    sql.extend_from_slice(table.as_bytes());
    sql.extend_from_slice(b" WHERE ID = ");
    sql.extend_from_slice(id.as_u64().to_string().as_bytes());
    sql.extend_from_slice(b" LIMIT 1");
    Sql::new(sql)
}

/// Compact insert of the caller-supplied cells.
#[must_use]
pub(crate) fn insert_row(table: &TableName, draft: &Draft) -> Sql {
    let mut sql = Vec::from(b"INSERT INTO ");
    sql.extend(tick(table.as_bytes()));
    sql.extend_from_slice(b" (");
    for (index, field) in Field::ALL.iter().enumerate() {
        if index > 0 {
            sql.extend_from_slice(b", ");
        }
        sql.extend(tick(field.column()));
    }
    sql.extend_from_slice(b") VALUES (");
    for (index, field) in Field::ALL.iter().enumerate() {
        if index > 0 {
            sql.extend_from_slice(b", ");
        }
        sql.extend(format_cell(*field, draft.get(*field)));
    }
    sql.push(b')');
    Sql::new(sql)
}

/// Compact update of the caller-supplied cells for `id`.
#[must_use]
pub(crate) fn update_row(table: &TableName, id: PostId, draft: &Draft) -> Sql {
    let mut sql = Vec::from(b"UPDATE ");
    sql.extend(tick(table.as_bytes()));
    sql.extend_from_slice(b" SET ");
    for (index, field) in Field::ALL.iter().enumerate() {
        if index > 0 {
            sql.extend_from_slice(b", ");
        }
        sql.extend(tick(field.column()));
        sql.extend_from_slice(b" = ");
        sql.extend(format_cell(*field, draft.get(*field)));
    }
    sql.extend_from_slice(b" WHERE ");
    sql.extend(tick(b"ID"));
    sql.extend_from_slice(b" = ");
    sql.extend_from_slice(id.as_u64().to_string().as_bytes());
    Sql::new(sql)
}

/// Remove one id.
#[must_use]
pub(crate) fn delete_row(table: &TableName, id: PostId) -> Sql {
    let mut sql = Vec::from(b"DELETE FROM ");
    sql.extend(tick(table.as_bytes()));
    sql.extend_from_slice(b" WHERE ");
    sql.extend(tick(b"ID"));
    sql.extend_from_slice(b" = ");
    sql.extend_from_slice(id.as_u64().to_string().as_bytes());
    Sql::new(sql)
}

fn tick(name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len() + 2);
    out.push(b'`');
    out.extend_from_slice(name);
    out.push(b'`');
    out
}

fn format_cell(field: Field, bytes: &[u8]) -> Vec<u8> {
    if field.is_integer() {
        format_int(bytes)
    } else {
        quote(bytes)
    }
}

fn format_int(bytes: &[u8]) -> Vec<u8> {
    match std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.parse::<i64>().ok())
    {
        Some(number) => number.to_string().into_bytes(),
        None => b"0".to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::{delete_row, format_int, insert_row, select_row, update_row};
    use crate::PostId;
    use crate::draft::{Draft, Field};
    use cms_db::{TablePrefix, TableStem};

    fn posts_table() -> Option<(cms_db::TableName, PostId)> {
        let prefix = TablePrefix::new("wp_");
        let stem = TableStem::new("posts");
        let id = PostId::new(12);
        if let (Ok(prefix), Ok(stem), Ok(id)) = (prefix, stem, id) {
            return Some((prefix.table(&stem), id));
        }
        None
    }

    fn sample_draft() -> Draft {
        let mut draft = Draft::new();
        draft
            .set(Field::Author, b"1")
            .set(Field::Date, b"2026-01-01 00:00:00")
            .set(Field::DateGmt, b"2026-01-01 00:00:00")
            .set(Field::Content, b"body")
            .set(Field::Title, b"Hello")
            .set(Field::Status, b"publish")
            .set(Field::TypeName, b"post")
            .set(Field::CommentStatus, b"open")
            .set(Field::PingStatus, b"open")
            .set(Field::Name, b"hello")
            .set(Field::Modified, b"2026-01-01 00:00:00")
            .set(Field::ModifiedGmt, b"2026-01-01 00:00:00");
        draft
    }

    #[test]
    fn statements_match_the_lab_prefix() {
        let opened = posts_table();
        assert!(opened.is_some());
        if let Some((table, id)) = opened {
            assert_eq!(
                select_row(&table, id).as_bytes(),
                b"SELECT * FROM wp_posts WHERE ID = 12 LIMIT 1"
            );
            let draft = sample_draft();
            assert_eq!(
                insert_row(&table, &draft).as_bytes(),
                b"INSERT INTO `wp_posts` (`post_author`, `post_date`, `post_date_gmt`, `post_content`, `post_content_filtered`, `post_title`, `post_excerpt`, `post_status`, `post_type`, `comment_status`, `ping_status`, `post_password`, `post_name`, `to_ping`, `pinged`, `post_modified`, `post_modified_gmt`, `post_parent`, `menu_order`, `post_mime_type`, `guid`) VALUES (1, '2026-01-01 00:00:00', '2026-01-01 00:00:00', 'body', '', 'Hello', '', 'publish', 'post', 'open', 'open', '', 'hello', '', '', '2026-01-01 00:00:00', '2026-01-01 00:00:00', 0, 0, '', '')"
            );
            assert_eq!(
                update_row(&table, id, &draft).as_bytes(),
                b"UPDATE `wp_posts` SET `post_author` = 1, `post_date` = '2026-01-01 00:00:00', `post_date_gmt` = '2026-01-01 00:00:00', `post_content` = 'body', `post_content_filtered` = '', `post_title` = 'Hello', `post_excerpt` = '', `post_status` = 'publish', `post_type` = 'post', `comment_status` = 'open', `ping_status` = 'open', `post_password` = '', `post_name` = 'hello', `to_ping` = '', `pinged` = '', `post_modified` = '2026-01-01 00:00:00', `post_modified_gmt` = '2026-01-01 00:00:00', `post_parent` = 0, `menu_order` = 0, `post_mime_type` = '', `guid` = '' WHERE `ID` = 12"
            );
            assert_eq!(
                delete_row(&table, id).as_bytes(),
                b"DELETE FROM `wp_posts` WHERE `ID` = 12"
            );
        }
    }

    #[test]
    fn integers_are_unquoted_and_junk_is_zero() {
        assert_eq!(format_int(b"7"), b"7");
        assert_eq!(format_int(b"01"), b"1");
        assert_eq!(format_int(b"-2"), b"-2");
        assert_eq!(format_int(b""), b"0");
        assert_eq!(format_int(b"x"), b"0");
        assert_eq!(format_int(&[0xff]), b"0");
        let opened = posts_table();
        assert!(opened.is_some());
        if let Some((table, _)) = opened {
            let mut draft = Draft::new();
            draft
                .set(Field::Author, b"nope")
                .set(Field::Title, b"a'b")
                .set(Field::Parent, b"3")
                .set(Field::MenuOrder, b"4");
            let sql = insert_row(&table, &draft);
            assert!(sql.as_bytes().starts_with(b"INSERT INTO `wp_posts`"));
            assert!(sql.as_bytes().windows(11).any(|w| w == b"VALUES (0, "));
            assert!(sql.as_bytes().windows(6).any(|w| w == br"'a\'b'"));
            assert!(sql.as_bytes().windows(6).any(|w| w == b", 3, 4"));
        }
    }

    proptest::proptest! {
        #[test]
        fn ids_are_decimal_and_unquoted(id in 1_u64..=10_000) {
            let prefix = TablePrefix::new("wp_");
            let stem = TableStem::new("posts");
            let post = PostId::new(id);
            proptest::prop_assert!(prefix.is_ok() && stem.is_ok() && post.is_ok());
            if let (Ok(prefix), Ok(stem), Ok(post)) = (prefix, stem, post) {
                let table = prefix.table(&stem);
                let select = select_row(&table, post);
                let expected = format!("SELECT * FROM wp_posts WHERE ID = {id} LIMIT 1");
                proptest::prop_assert_eq!(select.as_bytes(), expected.as_bytes());
                let delete = delete_row(&table, post);
                let expected = format!("DELETE FROM `wp_posts` WHERE `ID` = {id}");
                proptest::prop_assert_eq!(delete.as_bytes(), expected.as_bytes());
            }
        }
    }
}
