//! Primitive capability check (`contracts/auth/has.md`). SQL text was captured from the 7.1.3 lab.

use cms_auth::{Caps, Roles};
use cms_db::{Cell, Column, Reply, ResultSet, Row, Scripted, Sql, TablePrefix};
use cms_users::UserId;
use php_serialize::Writer;

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
    writer.array(2);
    writer.string(b"administrator");
    writer.bool(true);
    writer.string(b"do_not_allow");
    writer.bool(true);
    writer.end();
    writer.into_bytes()
}

fn opened() -> Option<(Caps<Scripted>, Roles<Scripted>, UserId)> {
    let roles_store =
        Scripted::new([(autoload_sql(), option_row("wp_user_roles", &admin_roles()))]);
    let caps_store = Scripted::new([(
        meta_sql(12),
        meta_row("12", "wp_capabilities", &stored_caps()),
    )]);
    let prefix = TablePrefix::new("wp_");
    let id = UserId::new(12);
    if let (Ok(prefix), Ok(id)) = (prefix, id) {
        let caps = Caps::new(caps_store, &prefix);
        let roles = Roles::new(roles_store, &prefix);
        if let (Ok(caps), Ok(roles)) = (caps, roles) {
            return Some((caps, roles, id));
        }
    }
    None
}

#[test]
fn exist_and_role_caps_are_allowed() {
    let bag = opened();
    assert!(bag.is_some());
    if let Some((mut caps, mut roles, id)) = bag {
        let table = roles.get();
        assert!(table.is_ok());
        assert!(
            table
                .as_ref()
                .is_ok_and(|table| table.is_role(b"administrator"))
        );
        if let Ok(table) = table {
            assert!(
                caps.has(id, &table, &[b"exist"])
                    .as_ref()
                    .is_ok_and(|allowed| *allowed)
            );
            assert!(
                caps.has(id, &table, &[b"manage_options"])
                    .as_ref()
                    .is_ok_and(|allowed| *allowed)
            );
            assert!(
                caps.has(id, &table, &[b"exist", b"manage_options"])
                    .as_ref()
                    .is_ok_and(|allowed| *allowed)
            );
        }
    }
}

#[test]
fn do_not_allow_and_missing_are_denied() {
    let bag = opened();
    assert!(bag.is_some());
    if let Some((mut caps, mut roles, id)) = bag {
        let table = roles.get();
        assert!(table.is_ok());
        if let Ok(table) = table {
            assert!(
                caps.has(id, &table, &[b"do_not_allow"])
                    .as_ref()
                    .is_ok_and(|allowed| !*allowed)
            );
            assert!(
                caps.has(id, &table, &[b"manage_options", b"do_not_allow"])
                    .as_ref()
                    .is_ok_and(|allowed| !*allowed)
            );
            assert!(
                caps.has(id, &table, &[b"edit_others_posts"])
                    .as_ref()
                    .is_ok_and(|allowed| !*allowed)
            );
        }
    }
}

#[test]
fn empty_required_list_is_allowed() {
    let bag = opened();
    assert!(bag.is_some());
    if let Some((mut caps, mut roles, id)) = bag {
        let table = roles.get();
        assert!(table.is_ok());
        if let Ok(table) = table {
            assert!(
                caps.has(id, &table, &[])
                    .as_ref()
                    .is_ok_and(|allowed| *allowed)
            );
        }
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let prefix = TablePrefix::new("wp_");
    let id = UserId::new(1);
    assert!(prefix.is_ok() && id.is_ok());
    if let (Ok(prefix), Ok(id)) = (prefix, id) {
        let caps = Caps::new(Scripted::new([]), &prefix);
        let roles = Roles::new(
            Scripted::new([(autoload_sql(), option_row("wp_user_roles", &admin_roles()))]),
            &prefix,
        );
        assert!(caps.is_ok() && roles.is_ok());
        if let (Ok(mut caps), Ok(mut roles)) = (caps, roles) {
            let table = roles.get();
            assert!(table.is_ok());
            if let Ok(table) = table {
                let value = caps.has(id, &table, &[b"exist"]);
                assert!(value.is_err());
                if let Err(error) = value {
                    assert!(error.is_unavailable());
                }
            }
        }
    }
}
