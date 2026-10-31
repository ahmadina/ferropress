//! The get-by-id statement the reference issues.

use cms_db::{Sql, TableName};

use crate::UserId;

/// One row: `SELECT * … WHERE ID = '{id}' LIMIT 1`.
#[must_use]
pub(crate) fn select_row(table: &TableName, id: UserId) -> Sql {
    let mut sql = Vec::from(b"SELECT * FROM ");
    sql.extend_from_slice(table.as_bytes());
    sql.extend_from_slice(b" WHERE ID = '");
    sql.extend_from_slice(id.as_u64().to_string().as_bytes());
    sql.extend_from_slice(b"' LIMIT 1");
    Sql::new(sql)
}

#[cfg(test)]
mod tests {
    use super::select_row;
    use crate::UserId;
    use cms_db::{TablePrefix, TableStem};

    #[test]
    fn statement_matches_the_lab_prefix() {
        let prefix = TablePrefix::new("wp_");
        let stem = TableStem::new("users");
        let id = UserId::new(12);
        assert!(prefix.is_ok() && stem.is_ok() && id.is_ok());
        if let (Ok(prefix), Ok(stem), Ok(id)) = (prefix, stem, id) {
            let table = prefix.table(&stem);
            assert_eq!(
                select_row(&table, id).as_bytes(),
                b"SELECT * FROM wp_users WHERE ID = '12' LIMIT 1"
            );
        }
    }

    proptest::proptest! {
        #[test]
        fn ids_are_quoted_decimals(id in 1_u64..=10_000) {
            let prefix = TablePrefix::new("wp_");
            let stem = TableStem::new("users");
            let user = UserId::new(id);
            proptest::prop_assert!(prefix.is_ok() && stem.is_ok() && user.is_ok());
            if let (Ok(prefix), Ok(stem), Ok(user)) = (prefix, stem, user) {
                let sql = select_row(&prefix.table(&stem), user);
                let expected = format!("SELECT * FROM wp_users WHERE ID = '{id}' LIMIT 1");
                proptest::prop_assert_eq!(sql.as_bytes(), expected.as_bytes());
            }
        }
    }
}
