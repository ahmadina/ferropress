//! Stored capability map (`contracts/auth/caps.md`). SQL text was captured from the 7.1.3 lab.

use cms_auth::{Caps, GrantMap};
use cms_db::{Cell, Column, Reply, ResultSet, Row, Scripted, Sql, TablePrefix};
use cms_users::UserId;

fn load_sql(id: u64) -> Sql {
    Sql::from(
        format!(
            "SELECT user_id, meta_key, meta_value FROM wp_usermeta WHERE user_id IN ({id}) ORDER BY umeta_id ASC"
        )
        .as_str(),
    )
}

fn row(user: &str, key: &str, value: &[u8]) -> Reply {
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

fn opened(store: Scripted) -> Option<(Caps<Scripted>, UserId)> {
    let prefix = TablePrefix::new("wp_");
    let id = UserId::new(12);
    if let (Ok(prefix), Ok(id)) = (prefix, id) {
        Caps::new(store, &prefix).ok().map(|caps| (caps, id))
    } else {
        None
    }
}

const ADMIN: &[u8] = b"a:2:{s:13:\"administrator\";b:1;s:10:\"edit_posts\";b:0;}";

#[test]
fn select_then_read_grants() {
    let store = Scripted::new([(load_sql(12), row("12", "wp_capabilities", ADMIN))]);
    let bag = opened(store);
    assert!(bag.is_some());
    if let Some((mut caps, id)) = bag {
        let first = caps.get(id);
        assert!(first.as_ref().is_ok_and(|map| {
            map.granted(b"administrator") && map.get(b"edit_posts") == Some(false)
        }));
        let again = caps.granted(id, b"administrator");
        assert!(again.as_ref().is_ok_and(|granted| *granted));
        assert!(
            caps.granted(id, b"manage_options")
                .as_ref()
                .is_ok_and(|granted| !granted)
        );
        let shown = format!("{caps:?}");
        assert!(shown.contains("Caps"));
        assert!(shown.contains("wp_capabilities"));
    }
}

#[test]
fn missing_or_junk_is_empty() {
    let store = Scripted::new([
        (load_sql(12), empty_rows()),
        (load_sql(13), row("13", "wp_capabilities", b"not-an-array")),
        (load_sql(14), row("14", "wp_capabilities", b"s:3:\"yes\";")),
    ]);
    let prefix = TablePrefix::new("wp_");
    let twelve = UserId::new(12);
    let thirteen = UserId::new(13);
    let fourteen = UserId::new(14);
    assert!(prefix.is_ok() && twelve.is_ok() && thirteen.is_ok() && fourteen.is_ok());
    if let (Ok(prefix), Ok(twelve), Ok(thirteen), Ok(fourteen)) =
        (prefix, twelve, thirteen, fourteen)
    {
        let opened = Caps::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut caps) = opened {
            assert!(caps.get(twelve).as_ref().is_ok_and(GrantMap::is_empty));
            assert!(caps.get(thirteen).as_ref().is_ok_and(GrantMap::is_empty));
            assert!(caps.get(fourteen).as_ref().is_ok_and(GrantMap::is_empty));
        }
    }
}

#[test]
fn an_empty_map_is_remembered() {
    let store = Scripted::new([(load_sql(99), empty_rows())]);
    let prefix = TablePrefix::new("wp_");
    let id = UserId::new(99);
    assert!(prefix.is_ok() && id.is_ok());
    if let (Ok(prefix), Ok(id)) = (prefix, id) {
        let opened = Caps::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut caps) = opened {
            assert!(caps.get(id).as_ref().is_ok_and(GrantMap::is_empty));
            assert!(
                caps.granted(id, b"administrator")
                    .as_ref()
                    .is_ok_and(|granted| !granted)
            );
        }
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let store = Scripted::new([]);
    let bag = opened(store);
    assert!(bag.is_some());
    if let Some((mut caps, id)) = bag {
        let value = caps.get(id);
        assert!(value.is_err());
        if let Err(error) = value {
            assert!(error.is_unavailable());
        }
    }
}

#[test]
fn a_custom_prefix_uses_that_capabilities_key() {
    let store = Scripted::new([(
        Sql::from(
            "SELECT user_id, meta_key, meta_value FROM fp_usermeta WHERE user_id IN (12) ORDER BY umeta_id ASC",
        ),
        row("12", "fp_capabilities", ADMIN),
    )]);
    let prefix = TablePrefix::new("fp_");
    let id = UserId::new(12);
    assert!(prefix.is_ok() && id.is_ok());
    if let (Ok(prefix), Ok(id)) = (prefix, id) {
        let opened = Caps::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut caps) = opened {
            assert!(
                caps.granted(id, b"administrator")
                    .as_ref()
                    .is_ok_and(|granted| *granted)
            );
        }
    }
}
