//! Option load and get (`contracts/options/load.md`). SQL text was captured from the 7.1.3 lab.

use cms_db::{Cell, Reply, Scripted, ServerError, Sql, TablePrefix};
use cms_options::{OptionName, Options};

fn autoload_sql() -> Sql {
    Sql::from(
        "SELECT option_name, option_value FROM wp_options WHERE autoload IN ( 'yes', 'on', 'auto-on', 'auto' )",
    )
}

fn all_sql() -> Sql {
    Sql::from("SELECT option_name, option_value FROM wp_options")
}

fn get_sql(option: &str) -> Sql {
    Sql::from(
        format!("SELECT option_value FROM wp_options WHERE option_name = '{option}' LIMIT 1")
            .as_str(),
    )
}

fn pair(option: &str, value: &str) -> cms_db::Row {
    cms_db::Row::new([Cell::bytes(option), Cell::bytes(value)])
}

fn columns() -> [cms_db::Column; 2] {
    [
        cms_db::Column::new("option_name"),
        cms_db::Column::new("option_value"),
    ]
}

#[test]
fn autoload_select_then_get_from_the_bag() {
    let store = Scripted::new([(
        autoload_sql(),
        Reply::Rows(cms_db::ResultSet::new(
            columns(),
            [
                pair("home", "http://example.test/"),
                pair("blogname", "Lab"),
            ],
        )),
    )]);
    let prefix = TablePrefix::new("wp_");
    let home_name = OptionName::new("home");
    let blog_name = OptionName::new("blogname");
    assert!(prefix.is_ok() && home_name.is_ok() && blog_name.is_ok());
    if let (Ok(prefix), Ok(home_name), Ok(blog_name)) = (prefix, home_name, blog_name) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            let home = options.get(&home_name);
            assert!(home.as_ref().is_ok_and(|value| {
                value.as_deref() == Some(b"http://example.test".as_slice())
            }));
            let blog = options.get(&blog_name);
            assert!(
                blog.as_ref()
                    .is_ok_and(|value| value.as_deref() == Some(b"Lab".as_slice()))
            );
            let shown = format!("{options:?}");
            assert!(shown.contains("Options"));
        }
    }
}

#[test]
fn empty_autoload_falls_back_to_every_row() {
    let store = Scripted::new([
        (
            autoload_sql(),
            Reply::Rows(cms_db::ResultSet::new(columns(), Vec::<cms_db::Row>::new())),
        ),
        (
            all_sql(),
            Reply::Rows(cms_db::ResultSet::new(
                columns(),
                [pair("siteurl", "http://example.test")],
            )),
        ),
    ]);
    let prefix = TablePrefix::new("wp_");
    let siteurl = OptionName::new("siteurl");
    assert!(prefix.is_ok() && siteurl.is_ok());
    if let (Ok(prefix), Ok(siteurl)) = (prefix, siteurl) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            let value = options.get(&siteurl);
            assert!(
                value
                    .as_ref()
                    .is_ok_and(|got| { got.as_deref() == Some(b"http://example.test".as_slice()) })
            );
        }
    }
}

#[test]
fn a_miss_is_remembered() {
    let store = Scripted::new([
        (
            autoload_sql(),
            Reply::Rows(cms_db::ResultSet::new(columns(), [pair("blogname", "Lab")])),
        ),
        (
            get_sql("missing"),
            Reply::Rows(cms_db::ResultSet::new(
                [cms_db::Column::new("option_value")],
                Vec::<cms_db::Row>::new(),
            )),
        ),
    ]);
    let prefix = TablePrefix::new("wp_");
    let missing = OptionName::new("missing");
    assert!(prefix.is_ok() && missing.is_ok());
    if let (Ok(prefix), Ok(missing)) = (prefix, missing) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            let first = options.get(&missing);
            assert!(first.as_ref().is_ok_and(Option::is_none));
            let second = options.get(&missing);
            assert!(second.as_ref().is_ok_and(Option::is_none));
        }
    }
}

#[test]
fn home_with_empty_bytes_uses_siteurl() {
    let store = Scripted::new([(
        autoload_sql(),
        Reply::Rows(cms_db::ResultSet::new(
            columns(),
            [pair("home", ""), pair("siteurl", "http://example.test/")],
        )),
    )]);
    let prefix = TablePrefix::new("wp_");
    let home_name = OptionName::new("home");
    assert!(prefix.is_ok() && home_name.is_ok());
    if let (Ok(prefix), Ok(home_name)) = (prefix, home_name) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            let home = options.get(&home_name);
            assert!(home.as_ref().is_ok_and(|value| {
                value.as_deref() == Some(b"http://example.test".as_slice())
            }));
        }
    }
}

#[test]
fn a_rejected_autoload_select_falls_back() {
    let store = Scripted::new([
        (
            autoload_sql(),
            Reply::Failed(ServerError::new(1146, "missing")),
        ),
        (
            all_sql(),
            Reply::Rows(cms_db::ResultSet::new(columns(), [pair("blogname", "Lab")])),
        ),
    ]);
    let prefix = TablePrefix::new("wp_");
    let blog = OptionName::new("blogname");
    assert!(prefix.is_ok() && blog.is_ok());
    if let (Ok(prefix), Ok(blog)) = (prefix, blog) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            let value = options.get(&blog);
            assert!(
                value
                    .as_ref()
                    .is_ok_and(|got| got.as_deref() == Some(b"Lab".as_slice()))
            );
        }
    }
}

#[test]
fn a_single_hit_is_cached() {
    let store = Scripted::new([
        (
            autoload_sql(),
            Reply::Rows(cms_db::ResultSet::new(columns(), [pair("blogname", "Lab")])),
        ),
        (
            get_sql("fresh_site"),
            Reply::single_column("option_value", [Cell::bytes("1")]),
        ),
    ]);
    let prefix = TablePrefix::new("wp_");
    let fresh = OptionName::new("fresh_site");
    assert!(prefix.is_ok() && fresh.is_ok());
    if let (Ok(prefix), Ok(fresh)) = (prefix, fresh) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            let first = options.get(&fresh);
            assert!(
                first
                    .as_ref()
                    .is_ok_and(|got| got.as_deref() == Some(b"1".as_slice()))
            );
            let second = options.get(&fresh);
            assert!(
                second
                    .as_ref()
                    .is_ok_and(|got| got.as_deref() == Some(b"1".as_slice()))
            );
        }
    }
}

#[test]
fn load_is_idempotent_and_skips_unusable_rows() {
    let store = Scripted::new([(
        autoload_sql(),
        Reply::Rows(cms_db::ResultSet::new(
            columns(),
            [
                cms_db::Row::new([Cell::Null, Cell::bytes("x")]),
                cms_db::Row::new([Cell::bytes("0"), Cell::bytes("zero")]),
                cms_db::Row::new([Cell::bytes("ok")]),
                pair("blogname", "Lab"),
                cms_db::Row::new([Cell::bytes("emptyval"), Cell::Null]),
            ],
        )),
    )]);
    let prefix = TablePrefix::new("wp_");
    let blog = OptionName::new("blogname");
    let emptyval = OptionName::new("emptyval");
    assert!(prefix.is_ok() && blog.is_ok() && emptyval.is_ok());
    if let (Ok(prefix), Ok(blog), Ok(emptyval)) = (prefix, blog, emptyval) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            assert!(options.load().is_ok());
            assert!(options.load().is_ok());
            assert!(
                options
                    .get(&blog)
                    .as_ref()
                    .is_ok_and(|got| got.as_deref() == Some(b"Lab".as_slice()))
            );
            assert!(
                options
                    .get(&emptyval)
                    .as_ref()
                    .is_ok_and(|got| got.as_deref() == Some(b"".as_slice()))
            );
        }
    }
}

#[test]
fn a_done_or_failed_get_is_a_miss() {
    let store = Scripted::new([
        (
            autoload_sql(),
            Reply::Rows(cms_db::ResultSet::new(columns(), [pair("blogname", "Lab")])),
        ),
        (
            get_sql("gone"),
            Reply::Done {
                affected: 0,
                insert_id: 0,
            },
        ),
    ]);
    let prefix = TablePrefix::new("wp_");
    let gone = OptionName::new("gone");
    assert!(prefix.is_ok() && gone.is_ok());
    if let (Ok(prefix), Ok(gone)) = (prefix, gone) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            let value = options.get(&gone);
            assert!(value.as_ref().is_ok_and(Option::is_none));
        }
    }
}

#[test]
fn both_load_queries_failing_leaves_an_empty_bag() {
    let store = Scripted::new([
        (
            autoload_sql(),
            Reply::Failed(ServerError::new(1146, "missing")),
        ),
        (all_sql(), Reply::Failed(ServerError::new(1146, "missing"))),
        (
            get_sql("blogname"),
            Reply::Rows(cms_db::ResultSet::new(
                [cms_db::Column::new("option_value")],
                Vec::<cms_db::Row>::new(),
            )),
        ),
    ]);
    let prefix = TablePrefix::new("wp_");
    let blog = OptionName::new("blogname");
    assert!(prefix.is_ok() && blog.is_ok());
    if let (Ok(prefix), Ok(blog)) = (prefix, blog) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            let value = options.get(&blog);
            assert!(value.as_ref().is_ok_and(Option::is_none));
        }
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let store = Scripted::new([]);
    let prefix = TablePrefix::new("wp_");
    let home_name = OptionName::new("home");
    assert!(prefix.is_ok() && home_name.is_ok());
    if let (Ok(prefix), Ok(home_name)) = (prefix, home_name) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            let value = options.get(&home_name);
            assert!(value.is_err());
            if let Err(error) = value {
                assert!(error.is_unavailable());
            }
        }
    }
}
