//! Current user (`contracts/auth/current.md`). SQL text was captured from the 7.1.3 lab.

use cms_auth::Current;
use cms_db::{Cell, Column, Reply, ResultSet, Row, Scripted, ServerError, Sql, TablePrefix};
use cms_users::UserId;

fn get_sql(id: u64) -> Sql {
    Sql::from(format!("SELECT * FROM wp_users WHERE ID = '{id}' LIMIT 1").as_str())
}

fn row(id: &str, login: &str) -> Reply {
    Reply::Rows(ResultSet::new(
        [Column::new("ID"), Column::new("user_login")],
        [Row::new([Cell::bytes(id), Cell::bytes(login)])],
    ))
}

fn opened(store: Scripted) -> Option<Current<Scripted>> {
    let prefix = TablePrefix::new("wp_");
    if let Ok(prefix) = prefix {
        Current::new(store, &prefix).ok()
    } else {
        None
    }
}

#[test]
fn unset_is_empty_without_sql() {
    let bag = opened(Scripted::new([]));
    assert!(bag.is_some());
    if let Some(current) = bag {
        assert!(current.get().is_none());
        assert!(current.id().is_none());
        let shown = format!("{current:?}");
        assert!(shown.contains("Current"));
    }
}

#[test]
fn set_loads_then_short_circuits() {
    let store = Scripted::new([(get_sql(12), row("12", "ada"))]);
    let id = UserId::new(12);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut current)) = (id, bag) {
        let first = current.set(id);
        assert!(first.as_ref().is_ok_and(|user| {
            user.as_ref()
                .is_some_and(|user| user.id() == id && user.login() == b"ada")
        }));
        let second = current.set(id);
        assert!(
            second
                .as_ref()
                .is_ok_and(|user| { user.as_ref().is_some_and(|user| user.login() == b"ada") })
        );
        assert!(current.id() == Some(id));
        assert!(current.get().is_some_and(|user| user.login() == b"ada"));
    }
}

#[test]
fn a_miss_leaves_the_current_user_empty() {
    let store = Scripted::new([
        (
            get_sql(99),
            Reply::Rows(ResultSet::new([Column::new("ID")], Vec::<Row>::new())),
        ),
        (
            get_sql(99),
            Reply::Failed(ServerError::new(1146, "missing")),
        ),
        (
            get_sql(99),
            Reply::Done {
                affected: 0,
                insert_id: 0,
            },
        ),
    ]);
    let id = UserId::new(99);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut current)) = (id, bag) {
        assert!(current.set(id).as_ref().is_ok_and(Option::is_none));
        assert!(current.get().is_none());
        assert!(current.set(id).as_ref().is_ok_and(Option::is_none));
        assert!(current.set(id).as_ref().is_ok_and(Option::is_none));
    }
}

#[test]
fn clear_empties_without_sql() {
    let store = Scripted::new([(get_sql(12), row("12", "ada"))]);
    let id = UserId::new(12);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut current)) = (id, bag) {
        assert!(current.set(id).as_ref().is_ok_and(Option::is_some));
        current.clear();
        assert!(current.get().is_none());
        assert!(current.id().is_none());
        current.clear();
        assert!(current.get().is_none());
    }
}

#[test]
fn switching_ids_loads_the_new_row() {
    let store = Scripted::new([
        (get_sql(12), row("12", "ada")),
        (get_sql(13), row("13", "al")),
    ]);
    let twelve = UserId::new(12);
    let thirteen = UserId::new(13);
    let bag = opened(store);
    assert!(twelve.is_ok() && thirteen.is_ok() && bag.is_some());
    if let (Ok(twelve), Ok(thirteen), Some(mut current)) = (twelve, thirteen, bag) {
        assert!(
            current
                .set(twelve)
                .as_ref()
                .is_ok_and(|user| { user.as_ref().is_some_and(|user| user.login() == b"ada") })
        );
        assert!(
            current
                .set(thirteen)
                .as_ref()
                .is_ok_and(|user| { user.as_ref().is_some_and(|user| user.login() == b"al") })
        );
        assert!(current.id() == Some(thirteen));
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let store = Scripted::new([]);
    let id = UserId::new(1);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut current)) = (id, bag) {
        let value = current.set(id);
        assert!(value.is_err());
        if let Err(error) = value {
            assert!(error.is_unavailable());
        }
    }
}
