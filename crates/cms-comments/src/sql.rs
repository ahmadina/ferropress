//! Statements the reference issues for comment reads and writes.

use cms_db::{Sql, TableName};

use crate::CommentId;
use crate::draft::{Draft, Field};
use crate::quote::quote;

/// One row: `SELECT * … WHERE comment_ID = {id} LIMIT 1`.
#[must_use]
pub(crate) fn select_row(table: &TableName, id: CommentId) -> Sql {
    let mut sql = Vec::from(b"SELECT * FROM ");
    sql.extend_from_slice(table.as_bytes());
    sql.extend_from_slice(b" WHERE comment_ID = ");
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
    for (index, field) in Field::INSERT.iter().enumerate() {
        if index > 0 {
            sql.extend_from_slice(b", ");
        }
        sql.extend(tick(field.column()));
    }
    sql.extend_from_slice(b") VALUES (");
    for (index, field) in Field::INSERT.iter().enumerate() {
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
pub(crate) fn update_row(table: &TableName, id: CommentId, draft: &Draft) -> Sql {
    let mut sql = Vec::from(b"UPDATE ");
    sql.extend(tick(table.as_bytes()));
    sql.extend_from_slice(b" SET ");
    for (index, field) in Field::UPDATE.iter().enumerate() {
        if index > 0 {
            sql.extend_from_slice(b", ");
        }
        sql.extend(tick(field.column()));
        sql.extend_from_slice(b" = ");
        sql.extend(format_cell(*field, draft.get(*field)));
    }
    sql.extend_from_slice(b" WHERE ");
    sql.extend(tick(b"comment_ID"));
    sql.extend_from_slice(b" = ");
    sql.extend_from_slice(id.as_u64().to_string().as_bytes());
    Sql::new(sql)
}

/// Remove one id.
#[must_use]
pub(crate) fn delete_row(table: &TableName, id: CommentId) -> Sql {
    let mut sql = Vec::from(b"DELETE FROM ");
    sql.extend(tick(table.as_bytes()));
    sql.extend_from_slice(b" WHERE ");
    sql.extend(tick(b"comment_ID"));
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
    use crate::CommentId;
    use crate::draft::{Draft, Field};
    use cms_db::{TablePrefix, TableStem};

    fn comments_table() -> Option<(cms_db::TableName, CommentId)> {
        let prefix = TablePrefix::new("wp_");
        let stem = TableStem::new("comments");
        let id = CommentId::new(12);
        if let (Ok(prefix), Ok(stem), Ok(id)) = (prefix, stem, id) {
            return Some((prefix.table(&stem), id));
        }
        None
    }

    fn sample_draft() -> Draft {
        let mut draft = Draft::new();
        draft
            .set(Field::Post, b"3")
            .set(Field::AuthorIp, b"203.0.113.4")
            .set(Field::Author, b"Ada")
            .set(Field::AuthorEmail, b"ada@example.test")
            .set(Field::AuthorUrl, b"https://ada.test")
            .set(Field::Date, b"2026-01-01 00:00:00")
            .set(Field::DateGmt, b"2026-01-01 00:00:00")
            .set(Field::Content, b"hello")
            .set(Field::Karma, b"0")
            .set(Field::Approved, b"1")
            .set(Field::Agent, b"agent")
            .set(Field::TypeName, b"comment")
            .set(Field::Parent, b"0")
            .set(Field::UserId, b"2");
        draft
    }

    #[test]
    fn statements_match_the_lab_prefix() {
        let opened = comments_table();
        assert!(opened.is_some());
        if let Some((table, id)) = opened {
            assert_eq!(
                select_row(&table, id).as_bytes(),
                b"SELECT * FROM wp_comments WHERE comment_ID = 12 LIMIT 1"
            );
            let draft = sample_draft();
            assert_eq!(
                insert_row(&table, &draft).as_bytes(),
                b"INSERT INTO `wp_comments` (`comment_post_ID`, `comment_author_IP`, `comment_author`, `comment_author_email`, `comment_author_url`, `comment_date`, `comment_date_gmt`, `comment_content`, `comment_karma`, `comment_approved`, `comment_agent`, `comment_type`, `comment_parent`, `user_id`) VALUES (3, '203.0.113.4', 'Ada', 'ada@example.test', 'https://ada.test', '2026-01-01 00:00:00', '2026-01-01 00:00:00', 'hello', 0, '1', 'agent', 'comment', 0, 2)"
            );
            assert_eq!(
                update_row(&table, id, &draft).as_bytes(),
                b"UPDATE `wp_comments` SET `comment_post_ID` = 3, `comment_author` = 'Ada', `comment_author_email` = 'ada@example.test', `comment_author_url` = 'https://ada.test', `comment_author_IP` = '203.0.113.4', `comment_date` = '2026-01-01 00:00:00', `comment_date_gmt` = '2026-01-01 00:00:00', `comment_content` = 'hello', `comment_karma` = 0, `comment_approved` = '1', `comment_agent` = 'agent', `comment_type` = 'comment', `comment_parent` = 0, `user_id` = 2 WHERE `comment_ID` = 12"
            );
            assert_eq!(
                delete_row(&table, id).as_bytes(),
                b"DELETE FROM `wp_comments` WHERE `comment_ID` = 12"
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
        let opened = comments_table();
        assert!(opened.is_some());
        if let Some((table, _)) = opened {
            let mut draft = Draft::new();
            draft
                .set(Field::Post, b"nope")
                .set(Field::Author, b"a'b")
                .set(Field::Karma, b"3")
                .set(Field::Parent, b"4")
                .set(Field::UserId, b"5");
            let sql = insert_row(&table, &draft);
            assert!(sql.as_bytes().starts_with(b"INSERT INTO `wp_comments`"));
            assert!(
                sql.as_bytes()
                    .windows(11)
                    .any(|window| window == b"VALUES (0, ")
            );
            assert!(sql.as_bytes().windows(6).any(|window| window == br"'a\'b'"));
            assert!(sql.as_bytes().windows(7).any(|window| window == b", 3, ''"));
            assert!(sql.as_bytes().ends_with(b", 4, 5)"));
        }
    }

    proptest::proptest! {
        #[test]
        fn ids_are_decimal_and_unquoted(id in 1_u64..=10_000) {
            let prefix = TablePrefix::new("wp_");
            let stem = TableStem::new("comments");
            let comment = CommentId::new(id);
            proptest::prop_assert!(prefix.is_ok() && stem.is_ok() && comment.is_ok());
            if let (Ok(prefix), Ok(stem), Ok(comment)) = (prefix, stem, comment) {
                let table = prefix.table(&stem);
                let select = select_row(&table, comment);
                let expected = format!("SELECT * FROM wp_comments WHERE comment_ID = {id} LIMIT 1");
                proptest::prop_assert_eq!(select.as_bytes(), expected.as_bytes());
                let delete = delete_row(&table, comment);
                let expected = format!("DELETE FROM `wp_comments` WHERE `comment_ID` = {id}");
                proptest::prop_assert_eq!(delete.as_bytes(), expected.as_bytes());
            }
        }
    }
}
