//! The get-by-id statement the reference issues.

use cms_db::{Sql, TableName};

use crate::PostId;

/// One row: `SELECT * … WHERE ID = {id} LIMIT 1`.
#[must_use]
pub(crate) fn select_row(table: &TableName, id: PostId) -> Sql {
    let mut sql = Vec::from(b"SELECT * FROM ");
    sql.extend_from_slice(table.as_bytes());
    sql.extend_from_slice(b" WHERE ID = ");
    sql.extend_from_slice(id.as_u64().to_string().as_bytes());
    sql.extend_from_slice(b" LIMIT 1");
    Sql::new(sql)
}

#[cfg(test)]
mod tests {
    use super::select_row;
    use crate::PostId;
    use cms_db::{TablePrefix, TableStem};

    #[test]
    fn statement_matches_the_lab_prefix() {
        let prefix = TablePrefix::new("wp_");
        let stem = TableStem::new("posts");
        let id = PostId::new(12);
        assert!(prefix.is_ok() && stem.is_ok() && id.is_ok());
        if let (Ok(prefix), Ok(stem), Ok(id)) = (prefix, stem, id) {
            let table = prefix.table(&stem);
            assert_eq!(
                select_row(&table, id).as_bytes(),
                b"SELECT * FROM wp_posts WHERE ID = 12 LIMIT 1"
            );
        }
    }

    proptest::proptest! {
        #[test]
        fn ids_are_decimal_and_unquoted(id in 1_u64..=10_000) {
            let prefix = TablePrefix::new("wp_");
            let stem = TableStem::new("posts");
            let post = PostId::new(id);
            proptest::prop_assert!(prefix.is_ok() && stem.is_ok() && post.is_ok());
            if let (Ok(prefix), Ok(stem), Ok(post)) = (prefix, stem, post) {
                let sql = select_row(&prefix.table(&stem), post);
                let expected = format!("SELECT * FROM wp_posts WHERE ID = {id} LIMIT 1");
                proptest::prop_assert_eq!(sql.as_bytes(), expected.as_bytes());
            }
        }
    }
}
