//! Option add, update and delete (`contracts/options/write.md`). SQL text was captured from the 7.1.3 lab.

use cms_db::{Cell, Reply, Scripted, ServerError, Sql, TablePrefix};
use cms_options::{Autoload, OptionName, Options, Write};

fn autoload_sql() -> Sql {
    Sql::from(
        "SELECT option_name, option_value FROM wp_options WHERE autoload IN ( 'yes', 'on', 'auto-on', 'auto' )",
    )
}

fn get_sql(option: &str) -> Sql {
    Sql::from(
        format!("SELECT option_value FROM wp_options WHERE option_name = '{option}' LIMIT 1")
            .as_str(),
    )
}

fn autoload_col_sql(option: &str) -> Sql {
    Sql::from(format!("SELECT autoload FROM wp_options WHERE option_name = '{option}'").as_str())
}

fn insert_sql(option: &str, value: &str, token: &str) -> Sql {
    Sql::from(
        format!(
            "INSERT INTO `wp_options` (`option_name`, `option_value`, `autoload`) VALUES ('{option}', '{value}', '{token}') ON DUPLICATE KEY UPDATE `option_name` = VALUES(`option_name`), `option_value` = VALUES(`option_value`), `autoload` = VALUES(`autoload`)"
        )
        .as_str(),
    )
}

fn update_value_sql(option: &str, value: &str) -> Sql {
    Sql::from(
        format!(
            "UPDATE `wp_options` SET `option_value` = '{value}' WHERE `option_name` = '{option}'"
        )
        .as_str(),
    )
}

fn update_both_sql(option: &str, value: &str, token: &str) -> Sql {
    Sql::from(
        format!(
            "UPDATE `wp_options` SET `option_value` = '{value}', `autoload` = '{token}' WHERE `option_name` = '{option}'"
        )
        .as_str(),
    )
}

fn delete_sql(option: &str) -> Sql {
    Sql::from(format!("DELETE FROM `wp_options` WHERE `option_name` = '{option}'").as_str())
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

fn bag(rows: impl Into<Vec<cms_db::Row>>) -> (Sql, Reply) {
    (
        autoload_sql(),
        Reply::Rows(cms_db::ResultSet::new(columns(), rows)),
    )
}

fn done(affected: u64) -> Reply {
    Reply::Done {
        affected,
        insert_id: 0,
    }
}

fn failed() -> Reply {
    Reply::Failed(ServerError::new(1146, "missing"))
}

fn empty_value() -> Reply {
    Reply::Rows(cms_db::ResultSet::new(
        [cms_db::Column::new("option_value")],
        Vec::<cms_db::Row>::new(),
    ))
}

fn value_hit(value: &str) -> Reply {
    Reply::single_column("option_value", [Cell::bytes(value)])
}

fn autoload_hit(token: &str) -> Reply {
    Reply::single_column("autoload", [Cell::bytes(token)])
}

fn autoload_empty() -> Reply {
    Reply::Rows(cms_db::ResultSet::new(
        [cms_db::Column::new("autoload")],
        Vec::<cms_db::Row>::new(),
    ))
}

#[test]
fn add_inserts_a_missing_loaded_name() {
    let store = Scripted::new([
        bag([pair("blogname", "Lab")]),
        (get_sql("fresh"), empty_value()),
        (insert_sql("fresh", "raw", "auto"), done(1)),
    ]);
    let prefix = TablePrefix::new("wp_");
    let fresh = OptionName::new("fresh");
    assert!(prefix.is_ok() && fresh.is_ok());
    if let (Ok(prefix), Ok(fresh)) = (prefix, fresh) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            let wrote = options.add(&fresh, b"raw", Autoload::Auto);
            assert!(wrote.as_ref().is_ok_and(|write| *write == Write::Changed));
            let value = options.get(&fresh);
            assert!(
                value
                    .as_ref()
                    .is_ok_and(|got| got.as_deref() == Some(b"raw".as_slice()))
            );
        }
    }
}

#[test]
fn add_skips_a_name_already_in_the_bag() {
    let store = Scripted::new([bag([pair("blogname", "Lab")])]);
    let prefix = TablePrefix::new("wp_");
    let blog = OptionName::new("blogname");
    assert!(prefix.is_ok() && blog.is_ok());
    if let (Ok(prefix), Ok(blog)) = (prefix, blog) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            let wrote = options.add(&blog, b"other", Autoload::On);
            assert!(wrote.as_ref().is_ok_and(|write| *write == Write::Unchanged));
        }
    }
}

#[test]
fn add_skips_a_single_name_hit() {
    let store = Scripted::new([
        bag([pair("blogname", "Lab")]),
        (get_sql("fresh_site"), value_hit("1")),
    ]);
    let prefix = TablePrefix::new("wp_");
    let fresh = OptionName::new("fresh_site");
    assert!(prefix.is_ok() && fresh.is_ok());
    if let (Ok(prefix), Ok(fresh)) = (prefix, fresh) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            let wrote = options.add(&fresh, b"2", Autoload::from_flag(true));
            assert!(wrote.as_ref().is_ok_and(|write| *write == Write::Unchanged));
        }
    }
}

#[test]
fn add_keeps_an_off_row_as_a_single() {
    let store = Scripted::new([
        bag([pair("blogname", "Lab")]),
        (get_sql("quiet"), empty_value()),
        (insert_sql("quiet", "n", "off"), done(1)),
    ]);
    let prefix = TablePrefix::new("wp_");
    let quiet = OptionName::new("quiet");
    assert!(prefix.is_ok() && quiet.is_ok());
    if let (Ok(prefix), Ok(quiet)) = (prefix, quiet) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            let wrote = options.add(&quiet, b"n", Autoload::from_flag(false));
            assert!(wrote.as_ref().is_ok_and(|write| *write == Write::Changed));
            let value = options.get(&quiet);
            assert!(
                value
                    .as_ref()
                    .is_ok_and(|got| got.as_deref() == Some(b"n".as_slice()))
            );
        }
    }
}

#[test]
fn add_treats_a_rejected_or_empty_insert_as_unchanged() {
    let store = Scripted::new([
        bag([pair("blogname", "Lab")]),
        (get_sql("a"), empty_value()),
        (insert_sql("a", "v", "on"), failed()),
        (get_sql("b"), empty_value()),
        (insert_sql("b", "v", "on"), done(0)),
        (get_sql("c"), empty_value()),
        (
            insert_sql("c", "v", "on"),
            Reply::single_column("x", [Cell::bytes("1")]),
        ),
    ]);
    let prefix = TablePrefix::new("wp_");
    let a = OptionName::new("a");
    let b = OptionName::new("b");
    let c = OptionName::new("c");
    assert!(prefix.is_ok() && a.is_ok() && b.is_ok() && c.is_ok());
    if let (Ok(prefix), Ok(a), Ok(b), Ok(c)) = (prefix, a, b, c) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            assert!(
                options
                    .add(&a, b"v", Autoload::On)
                    .as_ref()
                    .is_ok_and(|write| *write == Write::Unchanged)
            );
            assert!(
                options
                    .add(&b, b"v", Autoload::On)
                    .as_ref()
                    .is_ok_and(|write| *write == Write::Unchanged)
            );
            assert!(
                options
                    .add(&c, b"v", Autoload::On)
                    .as_ref()
                    .is_ok_and(|write| *write == Write::Changed)
            );
        }
    }
}

#[test]
fn update_rewrites_raw_bytes() {
    let store = Scripted::new([
        bag([pair("home", "http://example.test/")]),
        (update_both_sql("home", "http://other.test", "on"), done(1)),
    ]);
    let prefix = TablePrefix::new("wp_");
    let home = OptionName::new("home");
    assert!(prefix.is_ok() && home.is_ok());
    if let (Ok(prefix), Ok(home)) = (prefix, home) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            let same = options.update(&home, b"http://example.test/", Some(Autoload::On));
            assert!(same.as_ref().is_ok_and(|write| *write == Write::Unchanged));
            let wrote = options.update(&home, b"http://other.test", Some(Autoload::On));
            assert!(wrote.as_ref().is_ok_and(|write| *write == Write::Changed));
            let value = options.get(&home);
            assert!(
                value
                    .as_ref()
                    .is_ok_and(|got| { got.as_deref() == Some(b"http://other.test".as_slice()) })
            );
        }
    }
}

#[test]
fn update_of_a_missing_name_is_an_add() {
    let store = Scripted::new([
        bag([pair("blogname", "Lab")]),
        (get_sql("fresh"), empty_value()),
        (insert_sql("fresh", "v", "auto"), done(1)),
        (get_sql("quiet"), empty_value()),
        (insert_sql("quiet", "n", "off"), done(1)),
    ]);
    let prefix = TablePrefix::new("wp_");
    let fresh = OptionName::new("fresh");
    let quiet = OptionName::new("quiet");
    assert!(prefix.is_ok() && fresh.is_ok() && quiet.is_ok());
    if let (Ok(prefix), Ok(fresh), Ok(quiet)) = (prefix, fresh, quiet) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            let added = options.update(&fresh, b"v", None);
            assert!(added.as_ref().is_ok_and(|write| *write == Write::Changed));
            let off = options.update(&quiet, b"n", Some(Autoload::Off));
            assert!(off.as_ref().is_ok_and(|write| *write == Write::Changed));
        }
    }
}

#[test]
fn update_reevaluates_auto_family_tokens() {
    let store = Scripted::new([
        bag([pair("k", "v1")]),
        (autoload_col_sql("k"), autoload_hit("auto-on")),
        (update_both_sql("k", "v2", "auto"), done(1)),
        (autoload_col_sql("k"), autoload_hit("auto")),
        (update_value_sql("k", "v3"), done(1)),
        (autoload_col_sql("k"), autoload_hit("on")),
        (update_value_sql("k", "v4"), done(1)),
        (autoload_col_sql("k"), failed()),
        (update_value_sql("k", "v5"), done(1)),
    ]);
    let prefix = TablePrefix::new("wp_");
    let key = OptionName::new("k");
    assert!(prefix.is_ok() && key.is_ok());
    if let (Ok(prefix), Ok(key)) = (prefix, key) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            assert!(
                options
                    .update(&key, b"v2", None)
                    .as_ref()
                    .is_ok_and(|write| *write == Write::Changed)
            );
            assert!(
                options
                    .update(&key, b"v3", None)
                    .as_ref()
                    .is_ok_and(|write| *write == Write::Changed)
            );
            assert!(
                options
                    .update(&key, b"v4", None)
                    .as_ref()
                    .is_ok_and(|write| *write == Write::Changed)
            );
            assert!(
                options
                    .update(&key, b"v5", None)
                    .as_ref()
                    .is_ok_and(|write| *write == Write::Changed)
            );
            let value = options.get(&key);
            assert!(
                value
                    .as_ref()
                    .is_ok_and(|got| got.as_deref() == Some(b"v5".as_slice()))
            );
        }
    }
}

#[test]
fn update_of_a_single_without_a_new_token_stays_a_single() {
    let store = Scripted::new([
        bag([pair("blogname", "Lab")]),
        (get_sql("quiet"), value_hit("old")),
        (autoload_col_sql("quiet"), autoload_hit("off")),
        (update_value_sql("quiet", "new"), done(1)),
    ]);
    let prefix = TablePrefix::new("wp_");
    let quiet = OptionName::new("quiet");
    assert!(prefix.is_ok() && quiet.is_ok());
    if let (Ok(prefix), Ok(quiet)) = (prefix, quiet) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            let wrote = options.update(&quiet, b"new", None);
            assert!(wrote.as_ref().is_ok_and(|write| *write == Write::Changed));
            let value = options.get(&quiet);
            assert!(
                value
                    .as_ref()
                    .is_ok_and(|got| got.as_deref() == Some(b"new".as_slice()))
            );
        }
    }
}

#[test]
fn update_treats_a_rejected_write_as_unchanged() {
    let store = Scripted::new([
        bag([pair("k", "v1")]),
        (update_both_sql("k", "v2", "on"), failed()),
        (update_both_sql("k", "v3", "on"), done(0)),
    ]);
    let prefix = TablePrefix::new("wp_");
    let key = OptionName::new("k");
    assert!(prefix.is_ok() && key.is_ok());
    if let (Ok(prefix), Ok(key)) = (prefix, key) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            assert!(
                options
                    .update(&key, b"v2", Some(Autoload::On))
                    .as_ref()
                    .is_ok_and(|write| *write == Write::Unchanged)
            );
            assert!(
                options
                    .update(&key, b"v3", Some(Autoload::On))
                    .as_ref()
                    .is_ok_and(|write| *write == Write::Unchanged)
            );
            let value = options.get(&key);
            assert!(
                value
                    .as_ref()
                    .is_ok_and(|got| got.as_deref() == Some(b"v1".as_slice()))
            );
        }
    }
}

#[test]
fn delete_removes_a_loaded_row() {
    let store = Scripted::new([
        bag([pair("blogname", "Lab")]),
        (autoload_col_sql("blogname"), autoload_hit("yes")),
        (delete_sql("blogname"), done(1)),
    ]);
    let prefix = TablePrefix::new("wp_");
    let blog = OptionName::new("blogname");
    assert!(prefix.is_ok() && blog.is_ok());
    if let (Ok(prefix), Ok(blog)) = (prefix, blog) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            assert!(options.get(&blog).as_ref().is_ok_and(Option::is_some));
            let wrote = options.delete(&blog);
            assert!(wrote.as_ref().is_ok_and(|write| *write == Write::Changed));
            let again = options.get(&blog);
            assert!(again.as_ref().is_ok_and(Option::is_none));
        }
    }
}

#[test]
fn delete_without_a_prior_load_still_writes() {
    let store = Scripted::new([
        (autoload_col_sql("k"), autoload_hit("yes")),
        (delete_sql("k"), done(1)),
    ]);
    let prefix = TablePrefix::new("wp_");
    let key = OptionName::new("k");
    assert!(prefix.is_ok() && key.is_ok());
    if let (Ok(prefix), Ok(key)) = (prefix, key) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            let wrote = options.delete(&key);
            assert!(wrote.as_ref().is_ok_and(|write| *write == Write::Changed));
        }
    }
}

#[test]
fn delete_drops_a_single_and_skips_a_miss() {
    let store = Scripted::new([
        bag([pair("blogname", "Lab")]),
        (get_sql("quiet"), value_hit("n")),
        (autoload_col_sql("quiet"), autoload_hit("off")),
        (delete_sql("quiet"), done(1)),
        (autoload_col_sql("missing"), autoload_empty()),
        (autoload_col_sql("gone"), failed()),
        (
            autoload_col_sql("nulltok"),
            Reply::single_column("autoload", [Cell::Null]),
        ),
        (delete_sql("nulltok"), done(1)),
    ]);
    let prefix = TablePrefix::new("wp_");
    let quiet = OptionName::new("quiet");
    let missing = OptionName::new("missing");
    let gone = OptionName::new("gone");
    let nulltok = OptionName::new("nulltok");
    assert!(prefix.is_ok() && quiet.is_ok() && missing.is_ok() && gone.is_ok() && nulltok.is_ok());
    if let (Ok(prefix), Ok(quiet), Ok(missing), Ok(gone), Ok(nulltok)) =
        (prefix, quiet, missing, gone, nulltok)
    {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            assert!(
                options
                    .get(&quiet)
                    .as_ref()
                    .is_ok_and(|got| got.as_deref() == Some(b"n".as_slice()))
            );
            assert!(
                options
                    .delete(&quiet)
                    .as_ref()
                    .is_ok_and(|write| *write == Write::Changed)
            );
            assert!(options.get(&quiet).as_ref().is_ok_and(Option::is_none));
            assert!(
                options
                    .delete(&missing)
                    .as_ref()
                    .is_ok_and(|write| *write == Write::Unchanged)
            );
            assert!(
                options
                    .delete(&gone)
                    .as_ref()
                    .is_ok_and(|write| *write == Write::Unchanged)
            );
            assert!(
                options
                    .delete(&nulltok)
                    .as_ref()
                    .is_ok_and(|write| *write == Write::Changed)
            );
        }
    }
}

#[test]
fn delete_treats_a_rejected_or_empty_write_as_unchanged() {
    let store = Scripted::new([
        bag([pair("k", "v")]),
        (autoload_col_sql("k"), autoload_hit("on")),
        (delete_sql("k"), failed()),
        (autoload_col_sql("k"), autoload_hit("on")),
        (delete_sql("k"), done(0)),
    ]);
    let prefix = TablePrefix::new("wp_");
    let key = OptionName::new("k");
    assert!(prefix.is_ok() && key.is_ok());
    if let (Ok(prefix), Ok(key)) = (prefix, key) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            assert!(
                options
                    .get(&key)
                    .as_ref()
                    .is_ok_and(|got| got.as_deref() == Some(b"v".as_slice()))
            );
            assert!(
                options
                    .delete(&key)
                    .as_ref()
                    .is_ok_and(|write| *write == Write::Unchanged)
            );
            assert!(
                options
                    .delete(&key)
                    .as_ref()
                    .is_ok_and(|write| *write == Write::Unchanged)
            );
            assert!(
                options
                    .get(&key)
                    .as_ref()
                    .is_ok_and(|got| got.as_deref() == Some(b"v".as_slice()))
            );
        }
    }
}

#[test]
fn protected_names_are_not_written() {
    let store = Scripted::new([]);
    let prefix = TablePrefix::new("wp_");
    let all = OptionName::new("alloptions");
    let not = OptionName::new("notoptions");
    assert!(prefix.is_ok() && all.is_ok() && not.is_ok());
    if let (Ok(prefix), Ok(all), Ok(not)) = (prefix, all, not) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            assert!(
                options
                    .add(&all, b"x", Autoload::Auto)
                    .as_ref()
                    .is_ok_and(|write| *write == Write::Protected)
            );
            assert!(
                options
                    .update(&not, b"x", None)
                    .as_ref()
                    .is_ok_and(|write| *write == Write::Protected)
            );
            assert!(
                options
                    .delete(&all)
                    .as_ref()
                    .is_ok_and(|write| *write == Write::Protected)
            );
        }
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let store = Scripted::new([]);
    let prefix = TablePrefix::new("wp_");
    let name = OptionName::new("k");
    assert!(prefix.is_ok() && name.is_ok());
    if let (Ok(prefix), Ok(name)) = (prefix, name) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            let add = options.add(&name, b"v", Autoload::Auto);
            assert!(add.is_err());
            if let Err(error) = add {
                assert!(error.is_unavailable());
            }
        }
    }
}

#[test]
fn delete_unavailable_store_is_an_error() {
    let store = Scripted::new([]);
    let prefix = TablePrefix::new("wp_");
    let name = OptionName::new("k");
    assert!(prefix.is_ok() && name.is_ok());
    if let (Ok(prefix), Ok(name)) = (prefix, name) {
        let opened = Options::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut options) = opened {
            let delete = options.delete(&name);
            assert!(delete.is_err());
            if let Err(error) = delete {
                assert!(error.is_unavailable());
            }
        }
    }
}
