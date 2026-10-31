//! User insert, update and delete (`contracts/users/write.md`). SQL text was captured from the 7.1.3 lab.

use cms_db::{Cell, Column, Reply, ResultSet, Row, Scripted, ServerError, Sql, TablePrefix};
use cms_users::{Draft, Field, UserId, Users, Write};

fn get_sql(id: u64) -> Sql {
    Sql::from(format!("SELECT * FROM wp_users WHERE ID = '{id}' LIMIT 1").as_str())
}

fn insert_sql() -> Sql {
    Sql::from(
        "INSERT INTO `wp_users` (`user_pass`, `user_nicename`, `user_email`, `user_url`, `user_registered`, `user_activation_key`, `display_name`, `user_login`) VALUES ('hash', 'ada', 'ada@example.test', 'https://ada.test', '2026-01-01 00:00:00', '', 'Ada', 'ada')",
    )
}

fn update_sql(id: u64) -> Sql {
    Sql::from(
        format!(
            "UPDATE `wp_users` SET `user_pass` = 'hash', `user_nicename` = 'ada', `user_email` = 'ada@example.test', `user_url` = 'https://ada.test', `user_registered` = '2026-01-01 00:00:00', `user_activation_key` = '', `display_name` = 'Ada' WHERE `ID` = {id}"
        )
        .as_str(),
    )
}

fn delete_sql(id: u64) -> Sql {
    Sql::from(format!("DELETE FROM `wp_users` WHERE `ID` = {id}").as_str())
}

fn row(id: &str, login: &str) -> Reply {
    Reply::Rows(ResultSet::new(
        [Column::new("ID"), Column::new("user_login")],
        [Row::new([Cell::bytes(id), Cell::bytes(login)])],
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

fn opened(store: Scripted) -> Option<Users<Scripted>> {
    let prefix = TablePrefix::new("wp_");
    if let Ok(prefix) = prefix {
        Users::new(store, &prefix).ok()
    } else {
        None
    }
}

#[test]
fn insert_returns_the_new_id() {
    let store = Scripted::new([(insert_sql(), done(1, 42))]);
    let bag = opened(store);
    assert!(bag.is_some());
    if let Some(mut users) = bag {
        let written = users.insert(&sample_draft());
        assert!(written.is_ok());
        if let Ok(written) = written {
            assert!(written.changed());
            let id = written.inserted_id();
            assert!(id.is_some());
            if let Some(id) = id {
                assert_eq!(id.as_u64(), 42);
            }
        }
    }
}

#[test]
fn update_rewrites_and_forgets_a_hit() {
    let store = Scripted::new([
        (get_sql(12), row("12", "old")),
        (update_sql(12), done(1, 0)),
        (get_sql(12), row("12", "ada")),
    ]);
    let id = UserId::new(12);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut users)) = (id, bag) {
        let first = users.get(id);
        assert!(
            first
                .as_ref()
                .is_ok_and(|user| { user.as_ref().is_some_and(|user| user.login() == b"old") })
        );
        let written = users.update(id, &sample_draft());
        assert!(
            written
                .as_ref()
                .is_ok_and(|write| *write == Write::Replaced)
        );
        let second = users.get(id);
        assert!(
            second
                .as_ref()
                .is_ok_and(|user| { user.as_ref().is_some_and(|user| user.login() == b"ada") })
        );
    }
}

#[test]
fn delete_removes_and_forgets_a_hit() {
    let store = Scripted::new([
        (get_sql(12), row("12", "ada")),
        (delete_sql(12), done(1, 0)),
        (
            get_sql(12),
            Reply::Rows(ResultSet::new([Column::new("ID")], Vec::<Row>::new())),
        ),
    ]);
    let id = UserId::new(12);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut users)) = (id, bag) {
        assert!(users.get(id).as_ref().is_ok_and(Option::is_some));
        let written = users.delete(id);
        assert!(written.as_ref().is_ok_and(|write| *write == Write::Removed));
        assert!(users.get(id).as_ref().is_ok_and(Option::is_none));
    }
}

#[test]
fn failed_or_zero_is_unchanged() {
    let store = Scripted::new([
        (insert_sql(), done(1, 0)),
        (insert_sql(), failed()),
        (insert_sql(), row("1", "x")),
        (update_sql(12), done(0, 0)),
        (update_sql(12), failed()),
        (delete_sql(12), done(0, 0)),
        (delete_sql(12), failed()),
    ]);
    let id = UserId::new(12);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut users)) = (id, bag) {
        let draft = sample_draft();
        assert!(
            users
                .insert(&draft)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            users
                .insert(&draft)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            users
                .insert(&draft)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            users
                .update(id, &draft)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            users
                .update(id, &draft)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            users
                .delete(id)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            users
                .delete(id)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let store = Scripted::new([]);
    let id = UserId::new(1);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut users)) = (id, bag) {
        let draft = Draft::new();
        let inserted = users.insert(&draft);
        assert!(inserted.is_err());
        if let Err(error) = inserted {
            assert!(error.is_unavailable());
        }
        let updated = users.update(id, &draft);
        assert!(updated.is_err());
        if let Err(error) = updated {
            assert!(error.is_unavailable());
        }
        let deleted = users.delete(id);
        assert!(deleted.is_err());
        if let Err(error) = deleted {
            assert!(error.is_unavailable());
        }
    }
}
