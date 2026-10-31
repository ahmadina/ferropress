//! Current user capability check (`contracts/auth/current-can.md`).
//! SQL text was captured from the 7.1.3 lab.

use cms_auth::{Caps, Current, Roles};
use cms_db::{Cell, Column, Reply, ResultSet, Row, Scripted, Sql, TablePrefix};
use cms_users::UserId;
use php_serialize::Writer;

fn get_sql(id: u64) -> Sql {
    Sql::from(format!("SELECT * FROM wp_users WHERE ID = '{id}' LIMIT 1").as_str())
}

fn user_row(id: &str, login: &str) -> Reply {
    Reply::Rows(ResultSet::new(
        [Column::new("ID"), Column::new("user_login")],
        [Row::new([Cell::bytes(id), Cell::bytes(login)])],
    ))
}

fn admin_roles() -> Vec<u8> {
    let mut writer = Writer::new();
    writer.array(1);
    writer.string(b"administrator");
    writer.array(2);
    writer.string(b"name");
    writer.string(b"Administrator");
    writer.string(b"capabilities");
    writer.array(2);
    writer.string(b"manage_options");
    writer.bool(true);
    writer.string(b"edit_posts");
    writer.bool(true);
    writer.end();
    writer.end();
    writer.end();
    writer.into_bytes()
}

fn autoload_sql() -> Sql {
    Sql::from(
        "SELECT option_name, option_value FROM wp_options WHERE autoload IN ( 'yes', 'on', 'auto-on', 'auto' )",
    )
}

fn meta_sql(id: u64) -> Sql {
    Sql::from(
        format!(
            "SELECT user_id, meta_key, meta_value FROM wp_usermeta WHERE user_id IN ({id}) ORDER BY umeta_id ASC"
        )
        .as_str(),
    )
}

fn option_row(name: &str, value: &[u8]) -> Reply {
    Reply::Rows(ResultSet::new(
        [Column::new("option_name"), Column::new("option_value")],
        [Row::new([Cell::bytes(name), Cell::Bytes(value.into())])],
    ))
}

fn meta_row(user: &str, key: &str, value: &[u8]) -> Reply {
    Reply::Rows(ResultSet::new(
        [
            Column::new("user_id"),
            Column::new("meta_key"),
            Column::new("meta_value"),
        ],
        [Row::new([
            Cell::bytes(user),
            Cell::bytes(key),
            Cell::Bytes(value.into()),
        ])],
    ))
}

fn stored_caps() -> Vec<u8> {
    let mut writer = Writer::new();
    writer.array(1);
    writer.string(b"administrator");
    writer.bool(true);
    writer.end();
    writer.into_bytes()
}

fn prefix() -> Option<TablePrefix> {
    TablePrefix::new("wp_").ok()
}

fn empty_roles(prefix: &TablePrefix) -> Option<Roles<Scripted>> {
    Roles::new(
        Scripted::new([(autoload_sql(), option_row("wp_user_roles", &admin_roles()))]),
        prefix,
    )
    .ok()
}

#[test]
fn unset_is_anonymous_without_usermeta() {
    let prefix = prefix();
    assert!(prefix.is_some());
    if let Some(prefix) = prefix {
        let current = Current::new(Scripted::new([]), &prefix);
        let caps = Caps::new(Scripted::new([]), &prefix);
        let roles = empty_roles(&prefix);
        assert!(current.is_ok() && caps.is_ok() && roles.is_some());
        if let (Ok(current), Ok(mut caps), Some(mut roles)) = (current, caps, roles) {
            let table = roles.get();
            assert!(table.is_ok());
            if let Ok(table) = table {
                assert!(
                    current
                        .can(&mut caps, &table, b"exist")
                        .as_ref()
                        .is_ok_and(|allowed| *allowed)
                );
                assert!(
                    current
                        .can(&mut caps, &table, b"manage_options")
                        .as_ref()
                        .is_ok_and(|allowed| !*allowed)
                );
                assert!(
                    current
                        .can(&mut caps, &table, b"edit_user")
                        .as_ref()
                        .is_ok_and(|allowed| !*allowed)
                );
            }
        }
    }
}

#[test]
fn a_miss_is_anonymous() {
    let prefix = prefix();
    let id = UserId::new(99);
    assert!(prefix.is_some() && id.is_ok());
    if let (Some(prefix), Ok(id)) = (prefix, id) {
        let current = Current::new(
            Scripted::new([(
                get_sql(99),
                Reply::Rows(ResultSet::new([Column::new("ID")], Vec::<Row>::new())),
            )]),
            &prefix,
        );
        let caps = Caps::new(Scripted::new([]), &prefix);
        let roles = empty_roles(&prefix);
        assert!(current.is_ok() && caps.is_ok() && roles.is_some());
        if let (Ok(mut current), Ok(mut caps), Some(mut roles)) = (current, caps, roles) {
            assert!(current.set(id).as_ref().is_ok_and(Option::is_none));
            let table = roles.get();
            assert!(table.is_ok());
            if let Ok(table) = table {
                assert!(
                    current
                        .can(&mut caps, &table, b"exist")
                        .as_ref()
                        .is_ok_and(|allowed| *allowed)
                );
                assert!(
                    current
                        .can(&mut caps, &table, b"manage_options")
                        .as_ref()
                        .is_ok_and(|allowed| !*allowed)
                );
            }
        }
    }
}

#[test]
fn loaded_user_uses_mapped_allcaps() {
    let prefix = prefix();
    let id = UserId::new(12);
    assert!(prefix.is_some() && id.is_ok());
    if let (Some(prefix), Ok(id)) = (prefix, id) {
        let current = Current::new(
            Scripted::new([(get_sql(12), user_row("12", "ada"))]),
            &prefix,
        );
        let caps = Caps::new(
            Scripted::new([(
                meta_sql(12),
                meta_row("12", "wp_capabilities", &stored_caps()),
            )]),
            &prefix,
        );
        let roles = empty_roles(&prefix);
        assert!(current.is_ok() && caps.is_ok() && roles.is_some());
        if let (Ok(mut current), Ok(mut caps), Some(mut roles)) = (current, caps, roles) {
            assert!(current.set(id).as_ref().is_ok_and(Option::is_some));
            let table = roles.get();
            assert!(table.is_ok());
            if let Ok(table) = table {
                assert!(
                    current
                        .can(&mut caps, &table, b"manage_options")
                        .as_ref()
                        .is_ok_and(|allowed| *allowed)
                );
                assert!(
                    current
                        .can(&mut caps, &table, b"setup_network")
                        .as_ref()
                        .is_ok_and(|allowed| *allowed)
                );
                assert!(
                    current
                        .can(&mut caps, &table, b"customize")
                        .as_ref()
                        .is_ok_and(|allowed| !*allowed)
                );
                assert!(
                    current
                        .can(&mut caps, &table, b"delete_site")
                        .as_ref()
                        .is_ok_and(|allowed| !*allowed)
                );
            }
        }
    }
}

#[test]
fn unavailable_caps_store_is_an_error() {
    let prefix = prefix();
    let id = UserId::new(12);
    assert!(prefix.is_some() && id.is_ok());
    if let (Some(prefix), Ok(id)) = (prefix, id) {
        let current = Current::new(
            Scripted::new([(get_sql(12), user_row("12", "ada"))]),
            &prefix,
        );
        let caps = Caps::new(Scripted::new([]), &prefix);
        let roles = empty_roles(&prefix);
        assert!(current.is_ok() && caps.is_ok() && roles.is_some());
        if let (Ok(mut current), Ok(mut caps), Some(mut roles)) = (current, caps, roles) {
            assert!(current.set(id).as_ref().is_ok_and(Option::is_some));
            let table = roles.get();
            assert!(table.is_ok());
            if let Ok(table) = table {
                let value = current.can(&mut caps, &table, b"exist");
                assert!(value.is_err());
                if let Err(error) = value {
                    assert!(error.is_unavailable());
                }
            }
        }
    }
}
