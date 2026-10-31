//! User-meta get (`contracts/users/meta.md`). SQL text was captured from the 7.1.3 lab.

use cms_db::{Cell, Column, Reply, ResultSet, Row, Scripted, ServerError, Sql, TablePrefix};
use cms_users::{MetaKey, UserId, UserMeta};

fn meta_sql(id: u64) -> Sql {
    Sql::from(
        format!(
            "SELECT user_id, meta_key, meta_value FROM wp_usermeta WHERE user_id IN ({id}) ORDER BY umeta_id ASC"
        )
        .as_str(),
    )
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

fn empty_rows() -> Reply {
    Reply::Rows(ResultSet::new(
        [
            Column::new("user_id"),
            Column::new("meta_key"),
            Column::new("meta_value"),
        ],
        Vec::<Row>::new(),
    ))
}

#[test]
fn select_then_get_from_the_bag() {
    let store = Scripted::new([(
        meta_sql(12),
        cells(&[
            ("12", "nickname", "Ada"),
            ("12", "nickname", "Ada Two"),
            ("12", "first_name", "Ada"),
        ]),
    )]);
    let prefix = TablePrefix::new("wp_");
    let id = UserId::new(12);
    let nick = MetaKey::new("nickname");
    let first = MetaKey::new("first_name");
    let missing = MetaKey::new("last_name");
    assert!(prefix.is_ok() && id.is_ok() && nick.is_ok() && first.is_ok() && missing.is_ok());
    if let (Ok(prefix), Ok(id), Ok(nick), Ok(first), Ok(missing)) =
        (prefix, id, nick, first, missing)
    {
        let opened = UserMeta::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut meta) = opened {
            let values = meta.get(id, &nick);
            assert!(values.as_ref().is_ok_and(|values| {
                values.is_some_and(|values| {
                    values.len() == 2
                        && values[0].as_ref() == b"Ada"
                        && values[1].as_ref() == b"Ada Two"
                })
            }));
            let one = meta.first(id, &first);
            assert!(
                one.as_ref()
                    .is_ok_and(|value| value == &Some(b"Ada".as_slice()))
            );
            let none = meta.first(id, &missing);
            assert!(none.as_ref().is_ok_and(Option::is_none));
            let shown = format!("{meta:?}");
            assert!(shown.contains("UserMeta"));
        }
    }
}

#[test]
fn an_empty_bag_is_remembered() {
    let store = Scripted::new([(meta_sql(99), empty_rows())]);
    let prefix = TablePrefix::new("wp_");
    let id = UserId::new(99);
    let key = MetaKey::new("nickname");
    assert!(prefix.is_ok() && id.is_ok() && key.is_ok());
    if let (Ok(prefix), Ok(id), Ok(key)) = (prefix, id, key) {
        let opened = UserMeta::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut meta) = opened {
            let first = meta.get(id, &key);
            assert!(first.as_ref().is_ok_and(Option::is_none));
            let second = meta.get(id, &key);
            assert!(second.as_ref().is_ok_and(Option::is_none));
        }
    }
}

#[test]
fn null_values_become_empty_bytes() {
    let store = Scripted::new([(
        meta_sql(3),
        Reply::Rows(ResultSet::new(
            [
                Column::new("user_id"),
                Column::new("meta_key"),
                Column::new("meta_value"),
            ],
            [Row::new([
                Cell::bytes("3"),
                Cell::bytes("nickname"),
                Cell::Null,
            ])],
        )),
    )]);
    let prefix = TablePrefix::new("wp_");
    let id = UserId::new(3);
    let key = MetaKey::new("nickname");
    assert!(prefix.is_ok() && id.is_ok() && key.is_ok());
    if let (Ok(prefix), Ok(id), Ok(key)) = (prefix, id, key) {
        let opened = UserMeta::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut meta) = opened {
            let value = meta.first(id, &key);
            assert!(
                value
                    .as_ref()
                    .is_ok_and(|value| value == &Some(b"".as_slice()))
            );
        }
    }
}

#[test]
fn a_done_or_failed_load_is_an_empty_bag() {
    let store = Scripted::new([
        (
            meta_sql(4),
            Reply::Done {
                affected: 0,
                insert_id: 0,
            },
        ),
        (
            meta_sql(5),
            Reply::Failed(ServerError::new(1146, "missing")),
        ),
    ]);
    let prefix = TablePrefix::new("wp_");
    let four = UserId::new(4);
    let five = UserId::new(5);
    let key = MetaKey::new("nickname");
    assert!(prefix.is_ok() && four.is_ok() && five.is_ok() && key.is_ok());
    if let (Ok(prefix), Ok(four), Ok(five), Ok(key)) = (prefix, four, five, key) {
        let opened = UserMeta::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut meta) = opened {
            assert!(meta.get(four, &key).as_ref().is_ok_and(Option::is_none));
            assert!(meta.get(four, &key).as_ref().is_ok_and(Option::is_none));
            assert!(meta.get(five, &key).as_ref().is_ok_and(Option::is_none));
            assert!(meta.get(five, &key).as_ref().is_ok_and(Option::is_none));
        }
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let store = Scripted::new([]);
    let prefix = TablePrefix::new("wp_");
    let id = UserId::new(1);
    let key = MetaKey::new("nickname");
    assert!(prefix.is_ok() && id.is_ok() && key.is_ok());
    if let (Ok(prefix), Ok(id), Ok(key)) = (prefix, id, key) {
        let opened = UserMeta::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut meta) = opened {
            let value = meta.get(id, &key);
            assert!(value.is_err());
            if let Err(error) = value {
                assert!(error.is_unavailable());
            }
        }
    }
}

#[test]
fn distinct_users_issue_distinct_statements() {
    let store = Scripted::new([
        (meta_sql(7), cells(&[("7", "nickname", "Ada")])),
        (meta_sql(8), cells(&[("8", "nickname", "Grace")])),
    ]);
    let prefix = TablePrefix::new("wp_");
    let ada = UserId::new(7);
    let grace = UserId::new(8);
    let key = MetaKey::new("nickname");
    assert!(prefix.is_ok() && ada.is_ok() && grace.is_ok() && key.is_ok());
    if let (Ok(prefix), Ok(ada), Ok(grace), Ok(key)) = (prefix, ada, grace, key) {
        let opened = UserMeta::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut meta) = opened {
            let one = meta.first(ada, &key);
            assert!(
                one.as_ref()
                    .is_ok_and(|value| value == &Some(b"Ada".as_slice()))
            );
            let two = meta.first(grace, &key);
            assert!(
                two.as_ref()
                    .is_ok_and(|value| value == &Some(b"Grace".as_slice()))
            );
        }
    }
}
