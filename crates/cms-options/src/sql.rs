//! Statements the reference issues for option reads.

use cms_db::{Sql, TableName};

use crate::name::OptionName;
use crate::quote::quote;

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

#[cfg(test)]
mod tests {
    use super::{select_all, select_autoloaded, select_value};
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
            assert!(home.is_ok());
            if let Ok(home) = home {
                assert_eq!(
                    select_value(&table, &home).as_bytes(),
                    b"SELECT option_value FROM wp_options WHERE option_name = 'home' LIMIT 1"
                );
            }
        }
    }
}
