//! Statements the reference issues for term reads and writes.

use cms_db::{Sql, TableName};

use crate::draft::{Draft, Field};
use crate::quote::quote;
use crate::{ObjectId, TermId, TermTaxonomyId};

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

/// Compact insert of the terms-table cells.
#[must_use]
pub(crate) fn insert_term(table: &TableName, draft: &Draft) -> Sql {
    insert_named(
        table,
        &[Field::Name, Field::Slug, Field::TermGroup],
        draft,
        None,
    )
}

/// Compact insert of the taxonomy-table cells for `term_id`.
#[must_use]
pub(crate) fn insert_taxonomy(table: &TableName, term_id: TermId, draft: &Draft) -> Sql {
    insert_named(
        table,
        &[
            Field::Taxonomy,
            Field::Description,
            Field::Parent,
            Field::Count,
        ],
        draft,
        Some(term_id),
    )
}

/// Compact update of the terms-table cells for `id`.
#[must_use]
pub(crate) fn update_term(table: &TableName, id: TermId, draft: &Draft) -> Sql {
    let mut sql = Vec::from(b"UPDATE ");
    sql.extend(tick(table.as_bytes()));
    sql.extend_from_slice(b" SET ");
    append_set(
        &mut sql,
        &[Field::Name, Field::Slug, Field::TermGroup],
        draft,
    );
    sql.extend_from_slice(b" WHERE ");
    sql.extend(tick(b"term_id"));
    sql.extend_from_slice(b" = ");
    sql.extend_from_slice(id.as_u64().to_string().as_bytes());
    Sql::new(sql)
}

/// Compact update of the taxonomy-table cells for `taxonomy_id`.
#[must_use]
pub(crate) fn update_taxonomy(
    table: &TableName,
    term_id: TermId,
    taxonomy_id: TermTaxonomyId,
    draft: &Draft,
) -> Sql {
    let mut sql = Vec::from(b"UPDATE ");
    sql.extend(tick(table.as_bytes()));
    sql.extend_from_slice(b" SET ");
    sql.extend(tick(b"term_id"));
    sql.extend_from_slice(b" = ");
    sql.extend_from_slice(term_id.as_u64().to_string().as_bytes());
    sql.extend_from_slice(b", ");
    append_set(
        &mut sql,
        &[Field::Taxonomy, Field::Description, Field::Parent],
        draft,
    );
    sql.extend_from_slice(b" WHERE ");
    sql.extend(tick(b"term_taxonomy_id"));
    sql.extend_from_slice(b" = ");
    sql.extend_from_slice(taxonomy_id.as_u64().to_string().as_bytes());
    Sql::new(sql)
}

/// Remove one taxonomy row.
#[must_use]
pub(crate) fn delete_taxonomy(table: &TableName, id: TermTaxonomyId) -> Sql {
    delete_where(table, b"term_taxonomy_id", id.as_u64())
}

/// How many taxonomy rows still use `id`.
#[must_use]
pub(crate) fn count_taxonomies(table: &TableName, id: TermId) -> Sql {
    let mut sql = Vec::from(b"SELECT COUNT(*) FROM ");
    sql.extend_from_slice(table.as_bytes());
    sql.extend_from_slice(b" WHERE term_id = ");
    sql.extend_from_slice(id.as_u64().to_string().as_bytes());
    Sql::new(sql)
}

/// Remove one terms row.
#[must_use]
pub(crate) fn delete_term(table: &TableName, id: TermId) -> Sql {
    delete_where(table, b"term_id", id.as_u64())
}

/// Whether one object–taxonomy pair exists.
#[must_use]
pub(crate) fn select_link(table: &TableName, object: ObjectId, taxonomy_id: TermTaxonomyId) -> Sql {
    let mut sql = Vec::from(b"SELECT term_taxonomy_id FROM ");
    sql.extend_from_slice(table.as_bytes());
    sql.extend_from_slice(b" WHERE object_id = ");
    sql.extend_from_slice(object.as_u64().to_string().as_bytes());
    sql.extend_from_slice(b" AND term_taxonomy_id = ");
    sql.extend_from_slice(taxonomy_id.as_u64().to_string().as_bytes());
    Sql::new(sql)
}

/// Object ids that use one taxonomy row.
#[must_use]
pub(crate) fn select_objects(table: &TableName, taxonomy_id: TermTaxonomyId) -> Sql {
    let mut sql = Vec::from(b"SELECT object_id FROM ");
    sql.extend_from_slice(table.as_bytes());
    sql.extend_from_slice(b" WHERE term_taxonomy_id = ");
    sql.extend_from_slice(taxonomy_id.as_u64().to_string().as_bytes());
    Sql::new(sql)
}

/// Insert one object–taxonomy pair.
#[must_use]
pub(crate) fn insert_link(table: &TableName, object: ObjectId, taxonomy_id: TermTaxonomyId) -> Sql {
    let mut sql = Vec::from(b"INSERT INTO ");
    sql.extend(tick(table.as_bytes()));
    sql.extend_from_slice(b" (`object_id`, `term_taxonomy_id`) VALUES (");
    sql.extend_from_slice(object.as_u64().to_string().as_bytes());
    sql.extend_from_slice(b", ");
    sql.extend_from_slice(taxonomy_id.as_u64().to_string().as_bytes());
    sql.push(b')');
    Sql::new(sql)
}

/// Remove one object's listed taxonomy rows.
#[must_use]
pub(crate) fn delete_links(
    table: &TableName,
    object: ObjectId,
    taxonomy_ids: &[TermTaxonomyId],
) -> Sql {
    let mut sql = Vec::from(b"DELETE FROM ");
    sql.extend_from_slice(table.as_bytes());
    sql.extend_from_slice(b" WHERE object_id = ");
    sql.extend_from_slice(object.as_u64().to_string().as_bytes());
    sql.extend_from_slice(b" AND term_taxonomy_id IN (");
    for (index, id) in taxonomy_ids.iter().enumerate() {
        if index > 0 {
            sql.extend_from_slice(b", ");
        }
        sql.push(b'\'');
        sql.extend_from_slice(id.as_u64().to_string().as_bytes());
        sql.push(b'\'');
    }
    sql.push(b')');
    Sql::new(sql)
}

fn insert_named(
    table: &TableName,
    fields: &[Field],
    draft: &Draft,
    term_id: Option<TermId>,
) -> Sql {
    let mut sql = Vec::from(b"INSERT INTO ");
    sql.extend(tick(table.as_bytes()));
    sql.extend_from_slice(b" (");
    if term_id.is_some() {
        sql.extend(tick(b"term_id"));
        sql.extend_from_slice(b", ");
    }
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            sql.extend_from_slice(b", ");
        }
        sql.extend(tick(field.column()));
    }
    sql.extend_from_slice(b") VALUES (");
    if let Some(term_id) = term_id {
        sql.extend_from_slice(term_id.as_u64().to_string().as_bytes());
        sql.extend_from_slice(b", ");
    }
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            sql.extend_from_slice(b", ");
        }
        sql.extend(format_cell(*field, draft.get(*field)));
    }
    sql.push(b')');
    Sql::new(sql)
}

fn append_set(sql: &mut Vec<u8>, fields: &[Field], draft: &Draft) {
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            sql.extend_from_slice(b", ");
        }
        sql.extend(tick(field.column()));
        sql.extend_from_slice(b" = ");
        sql.extend(format_cell(*field, draft.get(*field)));
    }
}

fn delete_where(table: &TableName, column: &[u8], id: u64) -> Sql {
    let mut sql = Vec::from(b"DELETE FROM ");
    sql.extend(tick(table.as_bytes()));
    sql.extend_from_slice(b" WHERE ");
    sql.extend(tick(column));
    sql.extend_from_slice(b" = ");
    sql.extend_from_slice(id.to_string().as_bytes());
    Sql::new(sql)
}

fn tick(name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len() + 2);
    out.push(b'`');
    out.extend_from_slice(name);
    out.push(b'`');
    out
}

fn format_cell(field: Field, bytes: &[u8]) -> Vec<u8> {
    if field.is_integer() {
        format_int(bytes)
    } else {
        quote(bytes)
    }
}

fn format_int(bytes: &[u8]) -> Vec<u8> {
    match std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.parse::<i64>().ok())
    {
        Some(number) => number.to_string().into_bytes(),
        None => b"0".to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        count_taxonomies, delete_links, delete_taxonomy, delete_term, format_int, insert_link,
        insert_taxonomy, insert_term, select_link, select_objects, select_rows, update_taxonomy,
        update_term,
    };
    use crate::draft::{Draft, Field};
    use crate::{ObjectId, TermId, TermTaxonomyId};
    use cms_db::{TableName, TablePrefix, TableStem};

    fn tables() -> Option<(TableName, TableName, TermId, TermTaxonomyId)> {
        let prefix = TablePrefix::new("wp_");
        let terms = TableStem::new("terms");
        let taxonomy = TableStem::new("term_taxonomy");
        let term = TermId::new(12);
        let taxonomy_id = TermTaxonomyId::new(9);
        if let (Ok(prefix), Ok(terms), Ok(taxonomy), Ok(term), Ok(taxonomy_id)) =
            (prefix, terms, taxonomy, term, taxonomy_id)
        {
            return Some((
                prefix.table(&terms),
                prefix.table(&taxonomy),
                term,
                taxonomy_id,
            ));
        }
        None
    }

    fn sample_draft() -> Draft {
        let mut draft = Draft::new();
        draft
            .set(Field::Name, b"News")
            .set(Field::Slug, b"news")
            .set(Field::Taxonomy, b"category")
            .set(Field::Description, b"desc");
        draft
    }

    #[test]
    fn statements_match_the_lab_prefix() {
        let opened = tables();
        assert!(opened.is_some());
        if let Some((terms, taxonomy, term, taxonomy_id)) = opened {
            let draft = sample_draft();
            assert_eq!(
                select_rows(&terms, &taxonomy, term).as_bytes(),
                b"SELECT t.*, tt.* FROM wp_terms AS t INNER JOIN wp_term_taxonomy AS tt ON t.term_id = tt.term_id WHERE t.term_id = 12"
            );
            assert_eq!(
                insert_term(&terms, &draft).as_bytes(),
                b"INSERT INTO `wp_terms` (`name`, `slug`, `term_group`) VALUES ('News', 'news', 0)"
            );
            assert_eq!(
                insert_taxonomy(&taxonomy, term, &draft).as_bytes(),
                b"INSERT INTO `wp_term_taxonomy` (`term_id`, `taxonomy`, `description`, `parent`, `count`) VALUES (12, 'category', 'desc', 0, 0)"
            );
            assert_eq!(
                update_term(&terms, term, &draft).as_bytes(),
                b"UPDATE `wp_terms` SET `name` = 'News', `slug` = 'news', `term_group` = 0 WHERE `term_id` = 12"
            );
            assert_eq!(
                update_taxonomy(&taxonomy, term, taxonomy_id, &draft).as_bytes(),
                b"UPDATE `wp_term_taxonomy` SET `term_id` = 12, `taxonomy` = 'category', `description` = 'desc', `parent` = 0 WHERE `term_taxonomy_id` = 9"
            );
            assert_eq!(
                delete_taxonomy(&taxonomy, taxonomy_id).as_bytes(),
                b"DELETE FROM `wp_term_taxonomy` WHERE `term_taxonomy_id` = 9"
            );
            assert_eq!(
                count_taxonomies(&taxonomy, term).as_bytes(),
                b"SELECT COUNT(*) FROM wp_term_taxonomy WHERE term_id = 12"
            );
            assert_eq!(
                delete_term(&terms, term).as_bytes(),
                b"DELETE FROM `wp_terms` WHERE `term_id` = 12"
            );
            let prefix = TablePrefix::new("wp_");
            let rel = TableStem::new("term_relationships");
            let object = ObjectId::new(7);
            assert!(prefix.is_ok() && rel.is_ok() && object.is_ok());
            if let (Ok(prefix), Ok(rel), Ok(object)) = (prefix, rel, object) {
                let table = prefix.table(&rel);
                assert_eq!(
                    select_link(&table, object, taxonomy_id).as_bytes(),
                    b"SELECT term_taxonomy_id FROM wp_term_relationships WHERE object_id = 7 AND term_taxonomy_id = 9"
                );
                assert_eq!(
                    select_objects(&table, taxonomy_id).as_bytes(),
                    b"SELECT object_id FROM wp_term_relationships WHERE term_taxonomy_id = 9"
                );
                assert_eq!(
                    insert_link(&table, object, taxonomy_id).as_bytes(),
                    b"INSERT INTO `wp_term_relationships` (`object_id`, `term_taxonomy_id`) VALUES (7, 9)"
                );
                let twelve = TermTaxonomyId::new(12);
                assert!(twelve.is_ok());
                if let Ok(twelve) = twelve {
                    assert_eq!(
                        delete_links(&table, object, &[taxonomy_id, twelve]).as_bytes(),
                        b"DELETE FROM wp_term_relationships WHERE object_id = 7 AND term_taxonomy_id IN ('9', '12')"
                    );
                }
            }
        }
    }

    #[test]
    fn integers_are_unquoted_and_junk_is_zero() {
        assert_eq!(format_int(b"7"), b"7");
        assert_eq!(format_int(b"01"), b"1");
        assert_eq!(format_int(b"-2"), b"-2");
        assert_eq!(format_int(b""), b"0");
        assert_eq!(format_int(b"x"), b"0");
        assert_eq!(format_int(&[0xff]), b"0");
        let opened = tables();
        assert!(opened.is_some());
        if let Some((terms, _, _, _)) = opened {
            let mut draft = Draft::new();
            draft
                .set(Field::Name, b"a'b")
                .set(Field::TermGroup, b"nope");
            let sql = insert_term(&terms, &draft);
            assert!(sql.as_bytes().windows(6).any(|window| window == br"'a\'b'"));
            assert!(sql.as_bytes().ends_with(b", 0)"));
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
                let select = select_rows(&prefix.table(&terms), &prefix.table(&taxonomy), term);
                let expected = format!(
                    "SELECT t.*, tt.* FROM wp_terms AS t INNER JOIN wp_term_taxonomy AS tt ON t.term_id = tt.term_id WHERE t.term_id = {id}"
                );
                proptest::prop_assert_eq!(select.as_bytes(), expected.as_bytes());
                let count = count_taxonomies(&prefix.table(&taxonomy), term);
                let expected = format!("SELECT COUNT(*) FROM wp_term_taxonomy WHERE term_id = {id}");
                proptest::prop_assert_eq!(count.as_bytes(), expected.as_bytes());
            }
        }
    }
}
