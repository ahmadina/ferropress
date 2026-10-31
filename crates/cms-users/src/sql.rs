//! Statements the reference issues for user reads and writes.

use cms_db::{Sql, TableName};

use crate::UserId;
use crate::draft::{Draft, Field};
use crate::id::MetaId;
use crate::key::MetaKey;
use crate::quote::quote;

/// One user's meta rows: `SELECT … WHERE user_id IN ({id}) ORDER BY umeta_id`.
#[must_use]
pub(crate) fn select_meta(table: &TableName, id: UserId) -> Sql {
    let mut sql = Vec::from(b"SELECT user_id, meta_key, meta_value FROM ");
    sql.extend_from_slice(table.as_bytes());
    sql.extend_from_slice(b" WHERE user_id IN (");
    sql.extend_from_slice(id.as_u64().to_string().as_bytes());
    sql.extend_from_slice(b") ORDER BY umeta_id ASC");
    Sql::new(sql)
}

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

/// Insert one usermeta row: `user_id` `%d`, key and value `%s`.
#[must_use]
pub(crate) fn insert_meta(table: &TableName, id: UserId, key: &MetaKey, value: &[u8]) -> Sql {
    let mut sql = Vec::from(b"INSERT INTO ");
    sql.extend(tick(table.as_bytes()));
    sql.extend_from_slice(b" (`user_id`, `meta_key`, `meta_value`) VALUES (");
    sql.extend_from_slice(id.as_u64().to_string().as_bytes());
    sql.extend_from_slice(b", ");
    sql.extend(quote(key.as_bytes()));
    sql.extend_from_slice(b", ");
    sql.extend(quote(value));
    sql.push(b')');
    Sql::new(sql)
}

/// Rewrite `meta_value` for one user and key.
#[must_use]
pub(crate) fn update_meta(table: &TableName, id: UserId, key: &MetaKey, value: &[u8]) -> Sql {
    let mut sql = Vec::from(b"UPDATE ");
    sql.extend(tick(table.as_bytes()));
    sql.extend_from_slice(b" SET `meta_value` = ");
    sql.extend(quote(value));
    sql.extend_from_slice(b" WHERE `user_id` = ");
    sql.extend_from_slice(id.as_u64().to_string().as_bytes());
    sql.extend_from_slice(b" AND `meta_key` = ");
    sql.extend(quote(key.as_bytes()));
    Sql::new(sql)
}

/// Row ids for one user and key.
#[must_use]
pub(crate) fn select_meta_ids(table: &TableName, id: UserId, key: &MetaKey) -> Sql {
    let mut sql = Vec::from(b"SELECT umeta_id FROM ");
    sql.extend_from_slice(table.as_bytes());
    sql.extend_from_slice(b" WHERE meta_key = ");
    sql.extend(quote(key.as_bytes()));
    sql.extend_from_slice(b" AND user_id = ");
    sql.extend_from_slice(id.as_u64().to_string().as_bytes());
    Sql::new(sql)
}

/// Delete the selected `umeta_id` list. `ids` must not be empty.
#[must_use]
pub(crate) fn delete_meta_ids(table: &TableName, ids: &[MetaId]) -> Sql {
    let mut sql = Vec::from(b"DELETE FROM ");
    sql.extend_from_slice(table.as_bytes());
    sql.extend_from_slice(b" WHERE umeta_id IN( ");
    for (index, id) in ids.iter().enumerate() {
        if index > 0 {
            sql.push(b',');
        }
        sql.extend_from_slice(id.as_u64().to_string().as_bytes());
    }
    sql.extend_from_slice(b" )");
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
    use super::{
        delete_meta_ids, delete_row, insert_meta, insert_row, select_meta, select_meta_ids,
        select_row, update_meta, update_row,
    };
    use crate::UserId;
    use crate::draft::{Draft, Field};
    use crate::id::MetaId;
    use crate::key::MetaKey;
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
            let meta = TableStem::new("usermeta");
            assert!(meta.is_ok());
            if let Ok(meta) = meta {
                let prefix = TablePrefix::new("wp_");
                assert!(prefix.is_ok());
                if let Ok(prefix) = prefix {
                    assert_eq!(
                        select_meta(&prefix.table(&meta), id).as_bytes(),
                        b"SELECT user_id, meta_key, meta_value FROM wp_usermeta WHERE user_id IN (12) ORDER BY umeta_id ASC"
                    );
                    let key = MetaKey::new("nickname");
                    let mid = MetaId::new(7);
                    let other = MetaId::new(8);
                    assert!(key.is_ok() && mid.is_ok() && other.is_ok());
                    if let (Ok(key), Ok(mid), Ok(other)) = (key, mid, other) {
                        let table = prefix.table(&meta);
                        assert_eq!(
                            insert_meta(&table, id, &key, b"Ada").as_bytes(),
                            b"INSERT INTO `wp_usermeta` (`user_id`, `meta_key`, `meta_value`) VALUES (12, 'nickname', 'Ada')"
                        );
                        assert_eq!(
                            update_meta(&table, id, &key, b"Ada").as_bytes(),
                            b"UPDATE `wp_usermeta` SET `meta_value` = 'Ada' WHERE `user_id` = 12 AND `meta_key` = 'nickname'"
                        );
                        assert_eq!(
                            select_meta_ids(&table, id, &key).as_bytes(),
                            b"SELECT umeta_id FROM wp_usermeta WHERE meta_key = 'nickname' AND user_id = 12"
                        );
                        assert_eq!(
                            delete_meta_ids(&table, &[mid, other]).as_bytes(),
                            b"DELETE FROM wp_usermeta WHERE umeta_id IN( 7,8 )"
                        );
                    }
                }
            }
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
                let meta = TableStem::new("usermeta");
                proptest::prop_assert!(meta.is_ok());
                if let Ok(meta) = meta {
                    let select = select_meta(&prefix.table(&meta), user);
                    let expected = format!(
                        "SELECT user_id, meta_key, meta_value FROM wp_usermeta WHERE user_id IN ({id}) ORDER BY umeta_id ASC"
                    );
                    proptest::prop_assert_eq!(select.as_bytes(), expected.as_bytes());
                }
            }
        }
    }
}
