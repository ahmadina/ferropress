//! Link manager mapping (`contracts/auth/links.md`). SQL text was captured from the 7.1.3 lab.

use cms_auth::{Caps, Flags, Roles};
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
    writer.array(1);
    writer.string(b"manage_links");
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

fn links_sql() -> Sql {
    Sql::from(
        "SELECT option_value FROM wp_options WHERE option_name = 'link_manager_enabled' LIMIT 1",
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

fn option_rows(pairs: &[(&str, &[u8])]) -> Reply {
    Reply::Rows(ResultSet::new(
        [Column::new("option_name"), Column::new("option_value")],
        pairs
            .iter()
            .map(|(name, value)| Row::new([Cell::bytes(name), Cell::Bytes((*value).into())]))
            .collect::<Vec<_>>(),
    ))
}

fn value_row(value: &[u8]) -> Reply {
    Reply::Rows(ResultSet::new(
        [Column::new("option_value")],
        [Row::new([Cell::Bytes(value.into())])],
    ))
}

fn empty_value() -> Reply {
    Reply::Rows(ResultSet::new(
        [Column::new("option_value")],
        Vec::<Row>::new(),
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

#[test]
fn missing_or_empty_denies_manage_links() {
    let prefix = prefix();
    let id = UserId::new(12);
    assert!(prefix.is_some() && id.is_ok());
    if let (Some(prefix), Ok(id)) = (prefix, id) {
        let roles_bytes = admin_roles();
        let roles = Roles::new(
            Scripted::new([
                (
                    autoload_sql(),
                    option_rows(&[("wp_user_roles", roles_bytes.as_slice())]),
                ),
                (links_sql(), empty_value()),
            ]),
            &prefix,
        );
        let caps = Caps::new(
            Scripted::new([(
                meta_sql(12),
                meta_row("12", "wp_capabilities", &stored_caps()),
            )]),
            &prefix,
        );
        assert!(roles.is_ok() && caps.is_ok());
        if let (Ok(mut roles), Ok(mut caps)) = (roles, caps) {
            let flags = roles.flags();
            assert!(flags.as_ref().is_ok_and(|flags| *flags == Flags::off()));
            let table = roles.get();
            assert!(table.is_ok());
            if let (Ok(flags), Ok(table)) = (flags, table) {
                assert!(
                    caps.can_on(id, &table, b"manage_links", None, flags)
                        .as_ref()
                        .is_ok_and(|allowed| !*allowed)
                );
            }
        }
    }
}

#[test]
fn one_allows_when_the_role_grants_it() {
    let prefix = prefix();
    let id = UserId::new(12);
    assert!(prefix.is_some() && id.is_ok());
    if let (Some(prefix), Ok(id)) = (prefix, id) {
        let roles_bytes = admin_roles();
        let roles = Roles::new(
            Scripted::new([(
                autoload_sql(),
                option_rows(&[
                    ("wp_user_roles", roles_bytes.as_slice()),
                    ("link_manager_enabled", b"1"),
                ]),
            )]),
            &prefix,
        );
        let caps = Caps::new(
            Scripted::new([(
                meta_sql(12),
                meta_row("12", "wp_capabilities", &stored_caps()),
            )]),
            &prefix,
        );
        assert!(roles.is_ok() && caps.is_ok());
        if let (Ok(mut roles), Ok(mut caps)) = (roles, caps) {
            let flags = roles.flags();
            let table = roles.get();
            assert!(
                flags
                    .as_ref()
                    .is_ok_and(|flags| *flags == Flags::links(true))
            );
            assert!(table.is_ok());
            if let (Ok(flags), Ok(table)) = (flags, table) {
                assert!(
                    caps.can_on(id, &table, b"manage_links", None, flags)
                        .as_ref()
                        .is_ok_and(|allowed| *allowed)
                );
                assert!(
                    caps.can(id, &table, b"manage_links")
                        .as_ref()
                        .is_ok_and(|allowed| !*allowed)
                );
            }
        }
    }
}

#[test]
fn flags_are_remembered() {
    let prefix = prefix();
    assert!(prefix.is_some());
    if let Some(prefix) = prefix {
        let roles_bytes = admin_roles();
        let roles = Roles::new(
            Scripted::new([
                (
                    autoload_sql(),
                    option_rows(&[("wp_user_roles", roles_bytes.as_slice())]),
                ),
                (links_sql(), value_row(b"0")),
            ]),
            &prefix,
        );
        assert!(roles.is_ok());
        if let Ok(mut roles) = roles {
            let first = roles.flags();
            let second = roles.flags();
            assert!(first.as_ref().is_ok_and(|flags| *flags == Flags::off()));
            assert!(second.as_ref().is_ok_and(|flags| *flags == Flags::off()));
        }
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let prefix = prefix();
    assert!(prefix.is_some());
    if let Some(prefix) = prefix {
        let roles = Roles::new(Scripted::new([]), &prefix);
        assert!(roles.is_ok());
        if let Ok(mut roles) = roles {
            let value = roles.flags();
            assert!(value.is_err());
            if let Err(error) = value {
                assert!(error.is_unavailable());
            }
        }
    }
}
