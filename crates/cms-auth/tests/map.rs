//! Single-site capability mapping (`contracts/auth/map.md`).

use cms_auth::{Actor, Caps, Roles, map_meta};
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
    writer.array(1);
    writer.string(b"administrator");
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

fn assert_names(cap: &[u8], actor: Actor, expected: &[&[u8]]) {
    let mapped = map_meta(cap, actor);
    let names: Vec<&[u8]> = mapped.names().collect();
    assert_eq!(names, expected);
}

fn user_actor() -> Actor {
    let id = UserId::new(12);
    assert!(id.is_ok());
    if let Ok(id) = id {
        return Actor::User(id);
    }
    Actor::Anonymous
}

#[test]
fn aliases_remap_to_primitives() {
    let actor = user_actor();
    assert_names(b"customize", actor, &[b"edit_theme_options"]);
    assert_names(b"delete_site", actor, &[b"do_not_allow"]);
    assert_names(b"update_https", actor, &[b"manage_options", b"update_core"]);
    assert_names(b"edit_categories", actor, &[b"manage_categories"]);
    assert_names(b"assign_post_tags", actor, &[b"edit_posts"]);
    assert_names(b"edit_blocks", actor, &[b"edit_posts"]);
    assert_names(b"promote_user", actor, &[b"promote_users"]);
    assert_names(b"edit_user", actor, &[b"edit_users"]);
    assert_names(b"manage_options", actor, &[b"manage_options"]);
}

#[test]
fn anonymous_cannot_edit_users() {
    assert_names(b"edit_user", Actor::Anonymous, &[b"do_not_allow"]);
    assert_names(b"edit_users", Actor::Anonymous, &[b"do_not_allow"]);
    assert_names(b"customize", Actor::Anonymous, &[b"edit_theme_options"]);
}

#[test]
fn can_uses_mapped_names() {
    let bag = opened();
    assert!(bag.is_some());
    if let Some((mut caps, mut roles, id)) = bag {
        let table = roles.get();
        assert!(table.is_ok());
        if let Ok(table) = table {
            assert!(
                caps.can(id, &table, b"manage_options")
                    .as_ref()
                    .is_ok_and(|allowed| *allowed)
            );
            assert!(
                caps.can(id, &table, b"setup_network")
                    .as_ref()
                    .is_ok_and(|allowed| *allowed)
            );
            assert!(
                caps.can(id, &table, b"assign_categories")
                    .as_ref()
                    .is_ok_and(|allowed| *allowed)
            );
            assert!(
                caps.can(id, &table, b"customize")
                    .as_ref()
                    .is_ok_and(|allowed| !*allowed)
            );
            assert!(
                caps.can(id, &table, b"delete_site")
                    .as_ref()
                    .is_ok_and(|allowed| !*allowed)
            );
            assert!(
                caps.can(id, &table, b"update_https")
                    .as_ref()
                    .is_ok_and(|allowed| !*allowed)
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
                let value = caps.can(id, &table, b"exist");
                assert!(value.is_err());
                if let Err(error) = value {
                    assert!(error.is_unavailable());
                }
            }
        }
    }
}
