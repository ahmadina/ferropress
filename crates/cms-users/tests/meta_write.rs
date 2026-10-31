//! User-meta insert, update and delete (`contracts/users/meta-write.md`).
//! SQL text was captured from the 7.1.3 lab.

use cms_db::{Cell, Column, Reply, ResultSet, Row, Scripted, ServerError, Sql, TablePrefix};
use cms_users::{MetaKey, MetaWrite, UserId, UserMeta};

fn load_sql(id: u64) -> Sql {
    Sql::from(
        format!(
            "SELECT user_id, meta_key, meta_value FROM wp_usermeta WHERE user_id IN ({id}) ORDER BY umeta_id ASC"
        )
        .as_str(),
    )
}

fn insert_sql(id: u64, key: &str, value: &str) -> Sql {
    Sql::from(
        format!(
            "INSERT INTO `wp_usermeta` (`user_id`, `meta_key`, `meta_value`) VALUES ({id}, '{key}', '{value}')"
        )
        .as_str(),
    )
}

fn update_sql(id: u64, key: &str, value: &str) -> Sql {
    Sql::from(
        format!(
            "UPDATE `wp_usermeta` SET `meta_value` = '{value}' WHERE `user_id` = {id} AND `meta_key` = '{key}'"
        )
        .as_str(),
    )
}

fn select_ids_sql(id: u64, key: &str) -> Sql {
    Sql::from(
        format!("SELECT umeta_id FROM wp_usermeta WHERE meta_key = '{key}' AND user_id = {id}")
            .as_str(),
    )
}

fn delete_ids_sql(ids: &str) -> Sql {
    Sql::from(format!("DELETE FROM wp_usermeta WHERE umeta_id IN( {ids} )").as_str())
}

fn cells(rows: &[(&str, &str, &str)]) -> Reply {
    Reply::Rows(ResultSet::new(
        [
            Column::new("user_id"),
            Column::new("meta_key"),
            Column::new("meta_value"),
        ],
        rows.iter()
            .map(|(user, key, value)| {
                Row::new([Cell::bytes(*user), Cell::bytes(*key), Cell::bytes(*value)])
            })
            .collect::<Vec<_>>(),
    ))
}

fn ids(values: &[&str]) -> Reply {
    Reply::Rows(ResultSet::new(
        [Column::new("umeta_id")],
        values
            .iter()
            .map(|id| Row::new([Cell::bytes(*id)]))
            .collect::<Vec<_>>(),
    ))
}

fn done(affected: u64, insert_id: u64) -> Reply {
    Reply::Done {
        affected,
        insert_id,
    }
}

fn failed() -> Reply {
    Reply::Failed(ServerError::new(1146, "missing"))
}

fn opened(store: Scripted) -> Option<(UserMeta<Scripted>, UserId, MetaKey)> {
    let prefix = TablePrefix::new("wp_");
    let id = UserId::new(12);
    let key = MetaKey::new("nickname");
    if let (Ok(prefix), Ok(id), Ok(key)) = (prefix, id, key) {
        UserMeta::new(store, &prefix)
            .ok()
            .map(|meta| (meta, id, key))
    } else {
        None
    }
}

#[test]
fn insert_returns_the_new_id() {
    let store = Scripted::new([(insert_sql(12, "nickname", "Ada"), done(1, 42))]);
    let bag = opened(store);
    assert!(bag.is_some());
    if let Some((mut meta, id, key)) = bag {
        let written = meta.insert(id, &key, b"Ada");
        assert!(written.is_ok());
        if let Ok(written) = written {
            assert!(written.changed());
            let meta_id = written.inserted_id();
            assert!(meta_id.is_some());
            if let Some(meta_id) = meta_id {
                assert_eq!(meta_id.as_u64(), 42);
            }
        }
    }
}

#[test]
fn update_rewrites_and_forgets_the_bag() {
    let store = Scripted::new([
        (load_sql(12), cells(&[("12", "nickname", "old")])),
        (update_sql(12, "nickname", "Ada"), done(1, 0)),
        (load_sql(12), cells(&[("12", "nickname", "Ada")])),
    ]);
    let bag = opened(store);
    assert!(bag.is_some());
    if let Some((mut meta, id, key)) = bag {
        let first = meta.first(id, &key);
        assert!(
            first
                .as_ref()
                .is_ok_and(|value| value == &Some(b"old".as_slice()))
        );
        let written = meta.update(id, &key, b"Ada");
        assert!(
            written
                .as_ref()
                .is_ok_and(|write| *write == MetaWrite::Replaced)
        );
        let second = meta.first(id, &key);
        assert!(
            second
                .as_ref()
                .is_ok_and(|value| value == &Some(b"Ada".as_slice()))
        );
    }
}

#[test]
fn delete_selects_ids_then_removes() {
    let store = Scripted::new([
        (load_sql(12), cells(&[("12", "nickname", "Ada")])),
        (select_ids_sql(12, "nickname"), ids(&["7", "8"])),
        (delete_ids_sql("7,8"), done(2, 0)),
        (
            load_sql(12),
            Reply::Rows(ResultSet::new(
                [
                    Column::new("user_id"),
                    Column::new("meta_key"),
                    Column::new("meta_value"),
                ],
                Vec::<Row>::new(),
            )),
        ),
    ]);
    let bag = opened(store);
    assert!(bag.is_some());
    if let Some((mut meta, id, key)) = bag {
        assert!(meta.first(id, &key).as_ref().is_ok_and(Option::is_some));
        let written = meta.delete(id, &key);
        assert!(
            written
                .as_ref()
                .is_ok_and(|write| *write == MetaWrite::Removed)
        );
        assert!(meta.first(id, &key).as_ref().is_ok_and(Option::is_none));
    }
}

#[test]
fn failed_or_zero_is_unchanged() {
    let store = Scripted::new([
        (insert_sql(12, "nickname", "Ada"), done(1, 0)),
        (insert_sql(12, "nickname", "Ada"), failed()),
        (
            insert_sql(12, "nickname", "Ada"),
            cells(&[("12", "x", "y")]),
        ),
        (update_sql(12, "nickname", "Ada"), done(0, 0)),
        (update_sql(12, "nickname", "Ada"), failed()),
        (select_ids_sql(12, "nickname"), ids(&[])),
        (select_ids_sql(12, "nickname"), failed()),
        (select_ids_sql(12, "nickname"), ids(&["7"])),
        (delete_ids_sql("7"), done(0, 0)),
        (select_ids_sql(12, "nickname"), ids(&["7"])),
        (delete_ids_sql("7"), failed()),
    ]);
    let bag = opened(store);
    assert!(bag.is_some());
    if let Some((mut meta, id, key)) = bag {
        assert!(
            meta.insert(id, &key, b"Ada")
                .as_ref()
                .is_ok_and(|write| *write == MetaWrite::Unchanged)
        );
        assert!(
            meta.insert(id, &key, b"Ada")
                .as_ref()
                .is_ok_and(|write| *write == MetaWrite::Unchanged)
        );
        assert!(
            meta.insert(id, &key, b"Ada")
                .as_ref()
                .is_ok_and(|write| *write == MetaWrite::Unchanged)
        );
        assert!(
            meta.update(id, &key, b"Ada")
                .as_ref()
                .is_ok_and(|write| *write == MetaWrite::Unchanged)
        );
        assert!(
            meta.update(id, &key, b"Ada")
                .as_ref()
                .is_ok_and(|write| *write == MetaWrite::Unchanged)
        );
        assert!(
            meta.delete(id, &key)
                .as_ref()
                .is_ok_and(|write| *write == MetaWrite::Unchanged)
        );
        assert!(
            meta.delete(id, &key)
                .as_ref()
                .is_ok_and(|write| *write == MetaWrite::Unchanged)
        );
        assert!(
            meta.delete(id, &key)
                .as_ref()
                .is_ok_and(|write| *write == MetaWrite::Unchanged)
        );
        assert!(
            meta.delete(id, &key)
                .as_ref()
                .is_ok_and(|write| *write == MetaWrite::Unchanged)
        );
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let store = Scripted::new([]);
    let bag = opened(store);
    assert!(bag.is_some());
    if let Some((mut meta, id, key)) = bag {
        let inserted = meta.insert(id, &key, b"Ada");
        assert!(inserted.is_err());
        if let Err(error) = inserted {
            assert!(error.is_unavailable());
        }
        let updated = meta.update(id, &key, b"Ada");
        assert!(updated.is_err());
        if let Err(error) = updated {
            assert!(error.is_unavailable());
        }
        let deleted = meta.delete(id, &key);
        assert!(deleted.is_err());
        if let Err(error) = deleted {
            assert!(error.is_unavailable());
        }
    }
}
