//! Statements the reference issues for user reads and writes.

use cms_db::{Sql, TableName};

use crate::UserId;
use crate::draft::{Draft, Field};
use crate::quote::quote;

/// One row: `SELECT * … WHERE ID = '{id}' LIMIT 1`.
#[must_use]
pub(crate) fn select_row(table: &TableName, id: UserId) -> Sql {
    let mut sql = Vec::from(b"SELECT * FROM ");
    sql.extend_from_slice(table.as_bytes());
    sql.extend_from_slice(b" WHERE ID = '");
    sql.extend_from_slice(id.as_u64().to_string().as_bytes());
    sql.extend_from_slice(b"' LIMIT 1");
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
        sql.extend(quote(draft.get(*field)));
    }
    sql.push(b')');
    Sql::new(sql)
}

/// Compact update of the caller-supplied cells for `id`.
#[must_use]
pub(crate) fn update_row(table: &TableName, id: UserId, draft: &Draft) -> Sql {
    let mut sql = Vec::from(b"UPDATE ");
    sql.extend(tick(table.as_bytes()));
    sql.extend_from_slice(b" SET ");
    for (index, field) in Field::UPDATE.iter().enumerate() {
        if index > 0 {
            sql.extend_from_slice(b", ");
        }
        sql.extend(tick(field.column()));
        sql.extend_from_slice(b" = ");
        sql.extend(quote(draft.get(*field)));
    }
    sql.extend_from_slice(b" WHERE ");
    sql.extend(tick(b"ID"));
    sql.extend_from_slice(b" = ");
    sql.extend_from_slice(id.as_u64().to_string().as_bytes());
    Sql::new(sql)
}

/// Remove one id.
#[must_use]
pub(crate) fn delete_row(table: &TableName, id: UserId) -> Sql {
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

#[cfg(test)]
mod tests {
    use super::{delete_row, insert_row, select_row, update_row};
    use crate::UserId;
    use crate::draft::{Draft, Field};
    use cms_db::{TablePrefix, TableStem};

    fn users_table() -> Option<(cms_db::TableName, UserId)> {
        let prefix = TablePrefix::new("wp_");
        let stem = TableStem::new("users");
        let id = UserId::new(12);
        if let (Ok(prefix), Ok(stem), Ok(id)) = (prefix, stem, id) {
            return Some((prefix.table(&stem), id));
        }
        None
    }

    fn sample_draft() -> Draft {
        let mut draft = Draft::new();
        draft
            .set(Field::Password, b"hash")
            .set(Field::Nicename, b"ada")
            .set(Field::Email, b"ada@example.test")
            .set(Field::Url, b"https://ada.test")
            .set(Field::Registered, b"2026-01-01 00:00:00")
            .set(Field::ActivationKey, b"")
            .set(Field::DisplayName, b"Ada")
            .set(Field::Login, b"ada");
        draft
    }

    #[test]
    fn statements_match_the_lab_prefix() {
        let opened = users_table();
        assert!(opened.is_some());
        if let Some((table, id)) = opened {
            assert_eq!(
                select_row(&table, id).as_bytes(),
                b"SELECT * FROM wp_users WHERE ID = '12' LIMIT 1"
            );
            let draft = sample_draft();
            assert_eq!(
                insert_row(&table, &draft).as_bytes(),
                b"INSERT INTO `wp_users` (`user_pass`, `user_nicename`, `user_email`, `user_url`, `user_registered`, `user_activation_key`, `display_name`, `user_login`) VALUES ('hash', 'ada', 'ada@example.test', 'https://ada.test', '2026-01-01 00:00:00', '', 'Ada', 'ada')"
            );
            assert_eq!(
                update_row(&table, id, &draft).as_bytes(),
                b"UPDATE `wp_users` SET `user_pass` = 'hash', `user_nicename` = 'ada', `user_email` = 'ada@example.test', `user_url` = 'https://ada.test', `user_registered` = '2026-01-01 00:00:00', `user_activation_key` = '', `display_name` = 'Ada' WHERE `ID` = 12"
            );
            assert_eq!(
                delete_row(&table, id).as_bytes(),
                b"DELETE FROM `wp_users` WHERE `ID` = 12"
            );
        }
    }

    #[test]
    fn cells_are_quoted_and_login_is_insert_only() {
        let opened = users_table();
        assert!(opened.is_some());
        if let Some((table, id)) = opened {
            let mut draft = Draft::new();
            draft.set(Field::Login, b"a'b").set(Field::Nicename, b"x");
            let insert = insert_row(&table, &draft);
            assert!(insert.as_bytes().starts_with(b"INSERT INTO `wp_users`"));
            assert!(insert.as_bytes().ends_with(br", 'a\'b')"));
            let update = update_row(&table, id, &draft);
            assert!(update.as_bytes().starts_with(b"UPDATE `wp_users`"));
            assert!(
                !update
                    .as_bytes()
                    .windows(10)
                    .any(|window| window == b"user_login")
            );
            assert!(update.as_bytes().ends_with(b"WHERE `ID` = 12"));
        }
    }

    proptest::proptest! {
        #[test]
        fn ids_are_quoted_on_select_and_unquoted_on_delete(id in 1_u64..=10_000) {
            let prefix = TablePrefix::new("wp_");
            let stem = TableStem::new("users");
            let user = UserId::new(id);
            proptest::prop_assert!(prefix.is_ok() && stem.is_ok() && user.is_ok());
            if let (Ok(prefix), Ok(stem), Ok(user)) = (prefix, stem, user) {
                let table = prefix.table(&stem);
                let select = select_row(&table, user);
                let expected = format!("SELECT * FROM wp_users WHERE ID = '{id}' LIMIT 1");
                proptest::prop_assert_eq!(select.as_bytes(), expected.as_bytes());
                let delete = delete_row(&table, user);
                let expected = format!("DELETE FROM `wp_users` WHERE `ID` = {id}");
                proptest::prop_assert_eq!(delete.as_bytes(), expected.as_bytes());
            }
        }
    }
}
