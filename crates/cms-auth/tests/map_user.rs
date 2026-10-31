//! User-object capability mapping (`contracts/auth/map.md`).
//! SQL text was captured from the 7.1.3 lab.

use cms_auth::{Actor, Caps, Current, Roles, map_user};
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
    writer.array(1);
    writer.string(b"manage_options");
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

fn roles_bag(prefix: &TablePrefix) -> Option<Roles<Scripted>> {
    Roles::new(
        Scripted::new([(autoload_sql(), option_row("wp_user_roles", &admin_roles()))]),
        prefix,
    )
    .ok()
}

fn assert_names(cap: &[u8], actor: Actor, target: Option<UserId>, expected: &[&[u8]]) {
    let mapped = map_user(cap, actor, target);
    let names: Vec<&[u8]> = mapped.names().collect();
    assert_eq!(names, expected);
}

#[test]
fn self_edit_requires_no_primitives() {
    let id = UserId::new(12);
    let other = UserId::new(13);
    assert!(id.is_ok() && other.is_ok());
    if let (Ok(id), Ok(other)) = (id, other) {
        let actor = Actor::User(id);
        assert_names(b"edit_user", actor, Some(id), &[]);
        assert_names(b"edit_user", actor, Some(other), &[b"edit_users"]);
        assert_names(b"edit_users", actor, Some(id), &[b"edit_users"]);
        assert_names(b"remove_user", actor, Some(id), &[b"do_not_allow"]);
        assert_names(b"remove_user", actor, Some(other), &[b"remove_users"]);
    }
}

#[test]
fn can_user_allows_self_edit_without_edit_users() {
    let prefix = prefix();
    let id = UserId::new(12);
    let other = UserId::new(13);
    assert!(prefix.is_some() && id.is_ok() && other.is_ok());
    if let (Some(prefix), Ok(id), Ok(other)) = (prefix, id, other) {
        let caps = Caps::new(
            Scripted::new([(
                meta_sql(12),
                meta_row("12", "wp_capabilities", &stored_caps()),
            )]),
            &prefix,
        );
        let roles = roles_bag(&prefix);
        assert!(caps.is_ok() && roles.is_some());
        if let (Ok(mut caps), Some(mut roles)) = (caps, roles) {
            let table = roles.get();
            assert!(table.is_ok());
            if let Ok(table) = table {
                assert!(
                    caps.can_user(id, &table, b"edit_user", Some(id))
                        .as_ref()
                        .is_ok_and(|allowed| *allowed)
                );
                assert!(
                    caps.can_user(id, &table, b"edit_user", Some(other))
                        .as_ref()
                        .is_ok_and(|allowed| !*allowed)
                );
                assert!(
                    caps.can_user(id, &table, b"remove_user", Some(id))
                        .as_ref()
                        .is_ok_and(|allowed| !*allowed)
                );
            }
        }
    }
}

#[test]
fn current_can_user_self_and_anonymous() {
    let prefix = prefix();
    let id = UserId::new(12);
    let other = UserId::new(13);
    assert!(prefix.is_some() && id.is_ok() && other.is_ok());
    if let (Some(prefix), Ok(id), Ok(other)) = (prefix, id, other) {
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
        let roles = roles_bag(&prefix);
        assert!(current.is_ok() && caps.is_ok() && roles.is_some());
        if let (Ok(mut current), Ok(mut caps), Some(mut roles)) = (current, caps, roles) {
            let table = roles.get();
            assert!(table.is_ok());
            if let Ok(table) = table {
                assert!(
                    current
                        .can_user(&mut caps, &table, b"edit_user", Some(id))
                        .as_ref()
                        .is_ok_and(|allowed| !*allowed)
                );
                assert!(current.set(id).as_ref().is_ok_and(Option::is_some));
                assert!(
                    current
                        .can_user(&mut caps, &table, b"edit_user", Some(id))
                        .as_ref()
                        .is_ok_and(|allowed| *allowed)
                );
                assert!(
                    current
                        .can_user(&mut caps, &table, b"edit_user", Some(other))
                        .as_ref()
                        .is_ok_and(|allowed| !*allowed)
                );
            }
        }
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let prefix = prefix();
    let id = UserId::new(12);
    assert!(prefix.is_some() && id.is_ok());
    if let (Some(prefix), Ok(id)) = (prefix, id) {
        let caps = Caps::new(Scripted::new([]), &prefix);
        let roles = roles_bag(&prefix);
        assert!(caps.is_ok() && roles.is_some());
        if let (Ok(mut caps), Some(mut roles)) = (caps, roles) {
            let table = roles.get();
            assert!(table.is_ok());
            if let Ok(table) = table {
                let value = caps.can_user(id, &table, b"edit_user", Some(id));
                assert!(value.is_err());
                if let Err(error) = value {
                    assert!(error.is_unavailable());
                }
            }
        }
    }
}
