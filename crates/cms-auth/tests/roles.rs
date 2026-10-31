//! Role table and allcaps merge (`contracts/auth/roles.md`).
//! SQL text was captured from the 7.1.3 lab.

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

fn empty_options() -> Reply {
    Reply::Rows(ResultSet::new(
        [Column::new("option_name"), Column::new("option_value")],
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

const STORED: &[u8] = b"a:2:{s:13:\"administrator\";b:1;s:10:\"edit_posts\";b:0;}";

#[test]
fn autoload_then_merge_with_stored_caps() {
    let roles_store =
        Scripted::new([(autoload_sql(), option_row("wp_user_roles", &admin_roles()))]);
    let caps_store = Scripted::new([(meta_sql(12), meta_row("12", "wp_capabilities", STORED))]);
    let prefix = TablePrefix::new("wp_");
    let id = UserId::new(12);
    assert!(prefix.is_ok() && id.is_ok());
    if let (Ok(prefix), Ok(id)) = (prefix, id) {
        let roles = Roles::new(roles_store, &prefix);
        let caps = Caps::new(caps_store, &prefix);
        assert!(roles.is_ok() && caps.is_ok());
        if let (Ok(mut roles), Ok(mut caps)) = (roles, caps) {
            let table = roles.get();
            assert!(
                table
                    .as_ref()
                    .is_ok_and(|table| table.is_role(b"administrator"))
            );
            let again = roles.get();
            assert!(
                again
                    .as_ref()
                    .is_ok_and(|table| table.is_role(b"administrator"))
            );
            if let Ok(table) = table {
                let merged = caps.all(id, &table);
                assert!(merged.as_ref().is_ok_and(|map| {
                    map.granted(b"manage_options") && map.get(b"edit_posts") == Some(false)
                }));
            }
            let shown = format!("{roles:?}");
            assert!(shown.contains("Roles"));
            assert!(shown.contains("wp_user_roles"));
        }
    }
}

#[test]
fn missing_or_junk_is_empty() {
    let store = Scripted::new([
        (
            autoload_sql(),
            Reply::Rows(ResultSet::new(
                [Column::new("option_name"), Column::new("option_value")],
                [Row::new([
                    Cell::bytes("home"),
                    Cell::bytes("http://x.test/"),
                ])],
            )),
        ),
        (
            Sql::from(
                "SELECT option_value FROM wp_options WHERE option_name = 'wp_user_roles' LIMIT 1",
            ),
            Reply::Rows(ResultSet::new(
                [Column::new("option_value")],
                Vec::<Row>::new(),
            )),
        ),
    ]);
    let junk = Scripted::new([(autoload_sql(), option_row("wp_user_roles", b"not-an-array"))]);
    let prefix = TablePrefix::new("wp_");
    assert!(prefix.is_ok());
    if let Ok(prefix) = prefix {
        let missing = Roles::new(store, &prefix);
        let bad = Roles::new(junk, &prefix);
        assert!(missing.is_ok() && bad.is_ok());
        if let (Ok(mut missing), Ok(mut bad)) = (missing, bad) {
            assert!(
                missing
                    .get()
                    .as_ref()
                    .is_ok_and(cms_auth::RoleTable::is_empty)
            );
            assert!(bad.get().as_ref().is_ok_and(cms_auth::RoleTable::is_empty));
        }
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let prefix = TablePrefix::new("wp_");
    assert!(prefix.is_ok());
    if let Ok(prefix) = prefix {
        let opened = Roles::new(Scripted::new([]), &prefix);
        assert!(opened.is_ok());
        if let Ok(mut roles) = opened {
            let value = roles.get();
            assert!(value.is_err());
            if let Err(error) = value {
                assert!(error.is_unavailable());
            }
        }
    }
}

#[test]
fn empty_autoload_and_empty_all_then_miss() {
    let store = Scripted::new([
        (autoload_sql(), empty_options()),
        (
            Sql::from("SELECT option_name, option_value FROM wp_options"),
            empty_options(),
        ),
        (
            Sql::from(
                "SELECT option_value FROM wp_options WHERE option_name = 'wp_user_roles' LIMIT 1",
            ),
            Reply::Rows(ResultSet::new(
                [Column::new("option_value")],
                Vec::<Row>::new(),
            )),
        ),
    ]);
    let prefix = TablePrefix::new("wp_");
    assert!(prefix.is_ok());
    if let Ok(prefix) = prefix {
        let opened = Roles::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut roles) = opened {
            assert!(
                roles
                    .get()
                    .as_ref()
                    .is_ok_and(cms_auth::RoleTable::is_empty)
            );
            assert!(
                roles
                    .get()
                    .as_ref()
                    .is_ok_and(cms_auth::RoleTable::is_empty)
            );
        }
    }
}
