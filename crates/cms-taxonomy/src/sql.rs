//! The get-by-id join the reference issues.

use cms_db::{Sql, TableName};

use crate::TermId;

/// Joined `terms` / `term_taxonomy` rows for one id.
#[must_use]
pub(crate) fn select_rows(terms: &TableName, taxonomy: &TableName, id: TermId) -> Sql {
    let mut sql = Vec::from(b"SELECT t.*, tt.* FROM ");
    sql.extend_from_slice(terms.as_bytes());
    sql.extend_from_slice(b" AS t INNER JOIN ");
    sql.extend_from_slice(taxonomy.as_bytes());
    sql.extend_from_slice(b" AS tt ON t.term_id = tt.term_id WHERE t.term_id = ");
    sql.extend_from_slice(id.as_u64().to_string().as_bytes());
    Sql::new(sql)
}

#[cfg(test)]
mod tests {
    use super::select_rows;
    use crate::TermId;
    use cms_db::{TablePrefix, TableStem};

    #[test]
    fn statement_matches_the_lab_prefix() {
        let prefix = TablePrefix::new("wp_");
        let terms = TableStem::new("terms");
        let taxonomy = TableStem::new("term_taxonomy");
        let id = TermId::new(12);
        assert!(prefix.is_ok() && terms.is_ok() && taxonomy.is_ok() && id.is_ok());
        if let (Ok(prefix), Ok(terms), Ok(taxonomy), Ok(id)) = (prefix, terms, taxonomy, id) {
            assert_eq!(
                select_rows(&prefix.table(&terms), &prefix.table(&taxonomy), id).as_bytes(),
                b"SELECT t.*, tt.* FROM wp_terms AS t INNER JOIN wp_term_taxonomy AS tt ON t.term_id = tt.term_id WHERE t.term_id = 12"
            );
        }
    }

    proptest::proptest! {
        #[test]
        fn ids_are_decimal_and_unquoted(id in 1_u64..=10_000) {
            let prefix = TablePrefix::new("wp_");
            let terms = TableStem::new("terms");
            let taxonomy = TableStem::new("term_taxonomy");
            let term = TermId::new(id);
            proptest::prop_assert!(prefix.is_ok() && terms.is_ok() && taxonomy.is_ok() && term.is_ok());
            if let (Ok(prefix), Ok(terms), Ok(taxonomy), Ok(term)) = (prefix, terms, taxonomy, term) {
                let sql = select_rows(&prefix.table(&terms), &prefix.table(&taxonomy), term);
                let expected = format!(
                    "SELECT t.*, tt.* FROM wp_terms AS t INNER JOIN wp_term_taxonomy AS tt ON t.term_id = tt.term_id WHERE t.term_id = {id}"
                );
                proptest::prop_assert_eq!(sql.as_bytes(), expected.as_bytes());
            }
        }
    }
}
