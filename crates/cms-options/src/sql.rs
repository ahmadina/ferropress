//! Statements the reference issues for option reads and writes.

use cms_db::{Sql, TableName};

use crate::autoload::Autoload;
use crate::name::OptionName;
use crate::quote::quote;

fn tick(table: &TableName) -> Vec<u8> {
    let mut out = Vec::with_capacity(table.as_bytes().len() + 2);
    out.push(b'`');
    out.extend_from_slice(table.as_bytes());
    out.push(b'`');
    out
}

/// Autoloaded rows: `yes`, `on`, `auto-on`, `auto`.
#[must_use]
pub(crate) fn select_autoloaded(table: &TableName) -> Sql {
    let mut sql = Vec::from(b"SELECT option_name, option_value FROM ");
    sql.extend_from_slice(table.as_bytes());
    sql.extend_from_slice(b" WHERE autoload IN ( 'yes', 'on', 'auto-on', 'auto' )");
    Sql::new(sql)
}

/// Every row, used when the autoload select is empty or rejected.
#[must_use]
pub(crate) fn select_all(table: &TableName) -> Sql {
    let mut sql = Vec::from(b"SELECT option_name, option_value FROM ");
    sql.extend_from_slice(table.as_bytes());
    Sql::new(sql)
}

/// One stored value.
#[must_use]
pub(crate) fn select_value(table: &TableName, name: &OptionName) -> Sql {
    let mut sql = Vec::from(b"SELECT option_value FROM ");
    sql.extend_from_slice(table.as_bytes());
    sql.extend_from_slice(b" WHERE option_name = ");
    sql.extend(quote(name.as_bytes()));
    sql.extend_from_slice(b" LIMIT 1");
    Sql::new(sql)
}

/// Autoload column for one name (unquoted table, no `LIMIT`).
#[must_use]
pub(crate) fn select_autoload(table: &TableName, name: &OptionName) -> Sql {
    let mut sql = Vec::from(b"SELECT autoload FROM ");
    sql.extend_from_slice(table.as_bytes());
    sql.extend_from_slice(b" WHERE option_name = ");
    sql.extend(quote(name.as_bytes()));
    Sql::new(sql)
}

/// Insert or replace one row.
#[must_use]
pub(crate) fn insert_row(
    table: &TableName,
    name: &OptionName,
    value: &[u8],
    autoload: Autoload,
) -> Sql {
    let mut sql = Vec::from(b"INSERT INTO ");
    sql.extend(tick(table));
    sql.extend_from_slice(b" (`option_name`, `option_value`, `autoload`) VALUES (");
    sql.extend(quote(name.as_bytes()));
    sql.extend_from_slice(b", ");
    sql.extend(quote(value));
    sql.extend_from_slice(b", ");
    sql.extend(quote(autoload.as_bytes()));
    sql.extend_from_slice(b") ON DUPLICATE KEY UPDATE `option_name` = VALUES(`option_name`), `option_value` = VALUES(`option_value`), `autoload` = VALUES(`autoload`)");
    Sql::new(sql)
}

/// Write the value, and optionally the autoload token.
#[must_use]
pub(crate) fn update_row(
    table: &TableName,
    name: &OptionName,
    value: &[u8],
    autoload: Option<Autoload>,
) -> Sql {
    let mut sql = Vec::from(b"UPDATE ");
    sql.extend(tick(table));
    sql.extend_from_slice(b" SET `option_value` = ");
    sql.extend(quote(value));
    if let Some(autoload) = autoload {
        sql.extend_from_slice(b", `autoload` = ");
        sql.extend(quote(autoload.as_bytes()));
    }
    sql.extend_from_slice(b" WHERE `option_name` = ");
    sql.extend(quote(name.as_bytes()));
    Sql::new(sql)
}

/// Remove one name.
#[must_use]
pub(crate) fn delete_row(table: &TableName, name: &OptionName) -> Sql {
    let mut sql = Vec::from(b"DELETE FROM ");
    sql.extend(tick(table));
    sql.extend_from_slice(b" WHERE `option_name` = ");
    sql.extend(quote(name.as_bytes()));
    Sql::new(sql)
}

#[cfg(test)]
mod tests {
    use super::{
        delete_row, insert_row, select_all, select_autoload, select_autoloaded, select_value,
        update_row,
    };
    use crate::Autoload;
    use crate::OptionName;
    use cms_db::{TablePrefix, TableStem};

    #[test]
    fn statements_match_the_lab_prefix() {
        let prefix = TablePrefix::new("wp_");
        let stem = TableStem::new("options");
        assert!(prefix.is_ok() && stem.is_ok());
        if let (Ok(prefix), Ok(stem)) = (prefix, stem) {
            let table = prefix.table(&stem);
            assert_eq!(
                select_autoloaded(&table).as_bytes(),
                b"SELECT option_name, option_value FROM wp_options WHERE autoload IN ( 'yes', 'on', 'auto-on', 'auto' )"
            );
            assert_eq!(
                select_all(&table).as_bytes(),
                b"SELECT option_name, option_value FROM wp_options"
            );
            let home = OptionName::new("home");
            let key = OptionName::new("k");
            assert!(home.is_ok() && key.is_ok());
            if let (Ok(home), Ok(key)) = (home, key) {
                assert_eq!(
                    select_value(&table, &home).as_bytes(),
                    b"SELECT option_value FROM wp_options WHERE option_name = 'home' LIMIT 1"
                );
                assert_eq!(
                    select_autoload(&table, &key).as_bytes(),
                    b"SELECT autoload FROM wp_options WHERE option_name = 'k'"
                );
                assert_eq!(
                    insert_row(&table, &key, b"v", Autoload::Auto).as_bytes(),
                    b"INSERT INTO `wp_options` (`option_name`, `option_value`, `autoload`) VALUES ('k', 'v', 'auto') ON DUPLICATE KEY UPDATE `option_name` = VALUES(`option_name`), `option_value` = VALUES(`option_value`), `autoload` = VALUES(`autoload`)"
                );
                assert_eq!(
                    update_row(&table, &key, b"v3", None).as_bytes(),
                    b"UPDATE `wp_options` SET `option_value` = 'v3' WHERE `option_name` = 'k'"
                );
                assert_eq!(
                    update_row(&table, &key, b"v2", Some(Autoload::On)).as_bytes(),
                    b"UPDATE `wp_options` SET `option_value` = 'v2', `autoload` = 'on' WHERE `option_name` = 'k'"
                );
                assert_eq!(
                    delete_row(&table, &key).as_bytes(),
                    b"DELETE FROM `wp_options` WHERE `option_name` = 'k'"
                );
            }
        }
    }
}
