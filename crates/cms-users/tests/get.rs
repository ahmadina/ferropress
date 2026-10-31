//! User get by id (`contracts/users/get.md`). SQL text was captured from the 7.1.3 lab.

use cms_db::{Cell, Column, Reply, ResultSet, Row, Scripted, ServerError, Sql, TablePrefix};
use cms_users::{UserId, Users};

fn get_sql(id: u64) -> Sql {
    Sql::from(format!("SELECT * FROM wp_users WHERE ID = '{id}' LIMIT 1").as_str())
}

fn row(id: &str, login: &str, display_name: &str) -> Reply {
    Reply::Rows(ResultSet::new(
        [
            Column::new("ID"),
            Column::new("user_login"),
            Column::new("display_name"),
        ],
        [Row::new([
            Cell::bytes(id),
            Cell::bytes(login),
            Cell::bytes(display_name),
        ])],
    ))
}

#[test]
fn select_then_get_from_the_bag() {
    let store = Scripted::new([(get_sql(12), row("12", "ada", "Ada"))]);
    let prefix = TablePrefix::new("wp_");
    let id = UserId::new(12);
    assert!(prefix.is_ok() && id.is_ok());
    if let (Ok(prefix), Ok(id)) = (prefix, id) {
        let opened = Users::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut users) = opened {
            let first = users.get(id);
            assert!(first.as_ref().is_ok_and(|user| {
                user.as_ref().is_some_and(|user| {
                    user.id() == id && user.login() == b"ada" && user.display_name() == b"Ada"
                })
            }));
            let second = users.get(id);
            assert!(
                second
                    .as_ref()
                    .is_ok_and(|user| { user.as_ref().is_some_and(|user| user.login() == b"ada") })
            );
            let shown = format!("{users:?}");
            assert!(shown.contains("Users"));
        }
    }
}

#[test]
fn a_miss_is_not_remembered() {
    let store = Scripted::new([
        (
            get_sql(99),
            Reply::Rows(ResultSet::new([Column::new("ID")], Vec::<Row>::new())),
        ),
        (get_sql(99), row("99", "later", "Later")),
    ]);
    let prefix = TablePrefix::new("wp_");
    let id = UserId::new(99);
    assert!(prefix.is_ok() && id.is_ok());
    if let (Ok(prefix), Ok(id)) = (prefix, id) {
        let opened = Users::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut users) = opened {
            let first = users.get(id);
            assert!(first.as_ref().is_ok_and(Option::is_none));
            let second = users.get(id);
            assert!(
                second.as_ref().is_ok_and(|user| {
                    user.as_ref().is_some_and(|user| user.login() == b"later")
                })
            );
        }
    }
}

#[test]
fn null_cells_become_empty_bytes() {
    let store = Scripted::new([(
        get_sql(3),
        Reply::Rows(ResultSet::new(
            [
                Column::new("ID"),
                Column::new("user_login"),
                Column::new("user_email"),
            ],
            [Row::new([Cell::bytes("3"), Cell::Null, Cell::Null])],
        )),
    )]);
    let prefix = TablePrefix::new("wp_");
    let id = UserId::new(3);
    assert!(prefix.is_ok() && id.is_ok());
    if let (Ok(prefix), Ok(id)) = (prefix, id) {
        let opened = Users::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut users) = opened {
            let value = users.get(id);
            assert!(value.as_ref().is_ok_and(|user| {
                user.as_ref()
                    .is_some_and(|user| user.login().is_empty() && user.email().is_empty())
            }));
        }
    }
}

#[test]
fn a_done_or_failed_get_is_a_miss() {
    let store = Scripted::new([
        (
            get_sql(4),
            Reply::Done {
                affected: 0,
                insert_id: 0,
            },
        ),
        (get_sql(5), Reply::Failed(ServerError::new(1146, "missing"))),
        (
            get_sql(6),
            Reply::Rows(ResultSet::new(
                [Column::new("ID")],
                [Row::new([Cell::bytes("0")])],
            )),
        ),
    ]);
    let prefix = TablePrefix::new("wp_");
    let four = UserId::new(4);
    let five = UserId::new(5);
    let six = UserId::new(6);
    assert!(prefix.is_ok() && four.is_ok() && five.is_ok() && six.is_ok());
    if let (Ok(prefix), Ok(four), Ok(five), Ok(six)) = (prefix, four, five, six) {
        let opened = Users::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut users) = opened {
            assert!(users.get(four).as_ref().is_ok_and(Option::is_none));
            assert!(users.get(five).as_ref().is_ok_and(Option::is_none));
            assert!(users.get(six).as_ref().is_ok_and(Option::is_none));
        }
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let store = Scripted::new([]);
    let prefix = TablePrefix::new("wp_");
    let id = UserId::new(1);
    assert!(prefix.is_ok() && id.is_ok());
    if let (Ok(prefix), Ok(id)) = (prefix, id) {
        let opened = Users::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut users) = opened {
            let value = users.get(id);
            assert!(value.is_err());
            if let Err(error) = value {
                assert!(error.is_unavailable());
            }
        }
    }
}
