//! Term insert, update and delete (`contracts/taxonomy/write.md`). SQL text was captured from the 7.1.3 lab.

use cms_db::{Cell, Column, Reply, ResultSet, Row, Scripted, ServerError, Sql, TablePrefix};
use cms_taxonomy::{Draft, Field, TermId, TermTaxonomyId, Terms, Write};

fn get_sql(id: u64) -> Sql {
    Sql::from(
        format!(
            "SELECT t.*, tt.* FROM wp_terms AS t INNER JOIN wp_term_taxonomy AS tt ON t.term_id = tt.term_id WHERE t.term_id = {id}"
        )
        .as_str(),
    )
}

fn insert_term_sql() -> Sql {
    Sql::from("INSERT INTO `wp_terms` (`name`, `slug`, `term_group`) VALUES ('News', 'news', 0)")
}

fn insert_taxonomy_sql(term_id: u64) -> Sql {
    Sql::from(
        format!(
            "INSERT INTO `wp_term_taxonomy` (`term_id`, `taxonomy`, `description`, `parent`, `count`) VALUES ({term_id}, 'category', 'desc', 0, 0)"
        )
        .as_str(),
    )
}

fn update_term_sql(id: u64) -> Sql {
    Sql::from(
        format!(
            "UPDATE `wp_terms` SET `name` = 'News', `slug` = 'news', `term_group` = 0 WHERE `term_id` = {id}"
        )
        .as_str(),
    )
}

fn update_taxonomy_sql(term_id: u64, taxonomy_id: u64) -> Sql {
    Sql::from(
        format!(
            "UPDATE `wp_term_taxonomy` SET `term_id` = {term_id}, `taxonomy` = 'category', `description` = 'desc', `parent` = 0 WHERE `term_taxonomy_id` = {taxonomy_id}"
        )
        .as_str(),
    )
}

fn delete_taxonomy_sql(taxonomy_id: u64) -> Sql {
    Sql::from(
        format!("DELETE FROM `wp_term_taxonomy` WHERE `term_taxonomy_id` = {taxonomy_id}").as_str(),
    )
}

fn count_sql(id: u64) -> Sql {
    Sql::from(format!("SELECT COUNT(*) FROM wp_term_taxonomy WHERE term_id = {id}").as_str())
}

fn delete_term_sql(id: u64) -> Sql {
    Sql::from(format!("DELETE FROM `wp_terms` WHERE `term_id` = {id}").as_str())
}

fn count(value: &str) -> Reply {
    Reply::Rows(ResultSet::new(
        [Column::new("COUNT(*)")],
        [Row::new([Cell::bytes(value)])],
    ))
}

fn row(id: &str, name: &str) -> Reply {
    Reply::Rows(ResultSet::new(
        [
            Column::new("term_id"),
            Column::new("name"),
            Column::new("taxonomy"),
        ],
        [Row::new([
            Cell::bytes(id),
            Cell::bytes(name),
            Cell::bytes("category"),
        ])],
    ))
}

fn done(affected: u64, insert_id: u64) -> Reply {
    Reply::Done {
        affected,
        insert_id,
    }
}

fn failed() -> Reply {
    Reply::Failed(ServerError::new(1146, "missing"))
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

fn opened(store: Scripted) -> Option<Terms<Scripted>> {
    let prefix = TablePrefix::new("wp_");
    if let Ok(prefix) = prefix {
        Terms::new(store, &prefix).ok()
    } else {
        None
    }
}

#[test]
fn insert_returns_both_new_ids() {
    let store = Scripted::new([
        (insert_term_sql(), done(1, 42)),
        (insert_taxonomy_sql(42), done(1, 9)),
    ]);
    let bag = opened(store);
    assert!(bag.is_some());
    if let Some(mut terms) = bag {
        let written = terms.insert(&sample_draft());
        assert!(written.is_ok());
        if let Ok(written) = written {
            assert!(written.changed());
            let ids = written.inserted_ids();
            assert!(ids.is_some());
            if let Some((term_id, taxonomy_id)) = ids {
                assert_eq!(term_id.as_u64(), 42);
                assert_eq!(taxonomy_id.as_u64(), 9);
            }
        }
    }
}

#[test]
fn update_rewrites_and_forgets_a_hit() {
    let store = Scripted::new([
        (get_sql(12), row("12", "Old")),
        (update_term_sql(12), done(1, 0)),
        (update_taxonomy_sql(12, 9), done(1, 0)),
        (get_sql(12), row("12", "News")),
    ]);
    let id = TermId::new(12);
    let taxonomy_id = TermTaxonomyId::new(9);
    let bag = opened(store);
    assert!(id.is_ok() && taxonomy_id.is_ok() && bag.is_some());
    if let (Ok(id), Ok(taxonomy_id), Some(mut terms)) = (id, taxonomy_id, bag) {
        assert!(
            terms
                .get(id, b"")
                .as_ref()
                .is_ok_and(|found| { found.term().is_some_and(|term| term.name() == b"Old") })
        );
        let written = terms.update(id, taxonomy_id, &sample_draft());
        assert!(
            written
                .as_ref()
                .is_ok_and(|write| *write == Write::Replaced)
        );
        assert!(
            terms
                .get(id, b"")
                .as_ref()
                .is_ok_and(|found| { found.term().is_some_and(|term| term.name() == b"News") })
        );
    }
}

#[test]
fn delete_removes_a_last_taxonomy() {
    let store = Scripted::new([
        (get_sql(12), row("12", "News")),
        (delete_taxonomy_sql(9), done(1, 0)),
        (count_sql(12), count("0")),
        (delete_term_sql(12), done(1, 0)),
        (
            get_sql(12),
            Reply::Rows(ResultSet::new([Column::new("term_id")], Vec::<Row>::new())),
        ),
    ]);
    let id = TermId::new(12);
    let taxonomy_id = TermTaxonomyId::new(9);
    let bag = opened(store);
    assert!(id.is_ok() && taxonomy_id.is_ok() && bag.is_some());
    if let (Ok(id), Ok(taxonomy_id), Some(mut terms)) = (id, taxonomy_id, bag) {
        assert!(
            terms
                .get(id, b"")
                .as_ref()
                .is_ok_and(|found| found.term().is_some())
        );
        let written = terms.delete(id, taxonomy_id);
        assert!(written.as_ref().is_ok_and(|write| *write == Write::Removed));
        assert!(
            terms
                .get(id, b"")
                .as_ref()
                .is_ok_and(cms_taxonomy::Found::is_miss)
        );
    }
}

#[test]
fn delete_keeps_a_shared_term_row() {
    let store = Scripted::new([
        (delete_taxonomy_sql(9), done(1, 0)),
        (count_sql(12), count("1")),
    ]);
    let id = TermId::new(12);
    let taxonomy_id = TermTaxonomyId::new(9);
    let bag = opened(store);
    assert!(id.is_ok() && taxonomy_id.is_ok() && bag.is_some());
    if let (Ok(id), Ok(taxonomy_id), Some(mut terms)) = (id, taxonomy_id, bag) {
        let written = terms.delete(id, taxonomy_id);
        assert!(written.as_ref().is_ok_and(|write| *write == Write::Removed));
    }
}

#[test]
fn failed_count_still_deletes_the_terms_row() {
    let store = Scripted::new([
        (delete_taxonomy_sql(9), done(1, 0)),
        (count_sql(12), failed()),
        (delete_term_sql(12), done(1, 0)),
    ]);
    let id = TermId::new(12);
    let taxonomy_id = TermTaxonomyId::new(9);
    let bag = opened(store);
    assert!(id.is_ok() && taxonomy_id.is_ok() && bag.is_some());
    if let (Ok(id), Ok(taxonomy_id), Some(mut terms)) = (id, taxonomy_id, bag) {
        let written = terms.delete(id, taxonomy_id);
        assert!(written.as_ref().is_ok_and(|write| *write == Write::Removed));
    }
}

#[test]
fn unreadable_count_deletes_the_terms_row() {
    let empty = Reply::Rows(ResultSet::new([Column::new("COUNT(*)")], Vec::<Row>::new()));
    let null = Reply::Rows(ResultSet::new(
        [Column::new("COUNT(*)")],
        [Row::new([Cell::Null])],
    ));
    let junk = count("x");
    let store = Scripted::new([
        (delete_taxonomy_sql(9), done(1, 0)),
        (count_sql(12), empty),
        (delete_term_sql(12), done(1, 0)),
        (delete_taxonomy_sql(9), done(1, 0)),
        (count_sql(12), null),
        (delete_term_sql(12), done(1, 0)),
        (delete_taxonomy_sql(9), done(1, 0)),
        (count_sql(12), junk),
        (delete_term_sql(12), done(1, 0)),
    ]);
    let id = TermId::new(12);
    let taxonomy_id = TermTaxonomyId::new(9);
    let bag = opened(store);
    assert!(id.is_ok() && taxonomy_id.is_ok() && bag.is_some());
    if let (Ok(id), Ok(taxonomy_id), Some(mut terms)) = (id, taxonomy_id, bag) {
        assert!(
            terms
                .delete(id, taxonomy_id)
                .as_ref()
                .is_ok_and(|write| *write == Write::Removed)
        );
        assert!(
            terms
                .delete(id, taxonomy_id)
                .as_ref()
                .is_ok_and(|write| *write == Write::Removed)
        );
        assert!(
            terms
                .delete(id, taxonomy_id)
                .as_ref()
                .is_ok_and(|write| *write == Write::Removed)
        );
    }
}

#[test]
fn failed_or_zero_is_unchanged() {
    let store = Scripted::new([
        (insert_term_sql(), done(1, 0)),
        (insert_term_sql(), failed()),
        (insert_term_sql(), done(1, 42)),
        (insert_taxonomy_sql(42), failed()),
        (update_term_sql(12), done(0, 0)),
        (update_taxonomy_sql(12, 9), done(0, 0)),
        (delete_taxonomy_sql(9), done(0, 0)),
        (count_sql(12), count("2")),
    ]);
    let id = TermId::new(12);
    let taxonomy_id = TermTaxonomyId::new(9);
    let bag = opened(store);
    assert!(id.is_ok() && taxonomy_id.is_ok() && bag.is_some());
    if let (Ok(id), Ok(taxonomy_id), Some(mut terms)) = (id, taxonomy_id, bag) {
        let draft = sample_draft();
        assert!(
            terms
                .insert(&draft)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            terms
                .insert(&draft)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            terms
                .insert(&draft)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            terms
                .update(id, taxonomy_id, &draft)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            terms
                .delete(id, taxonomy_id)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let store = Scripted::new([]);
    let id = TermId::new(1);
    let taxonomy_id = TermTaxonomyId::new(2);
    let bag = opened(store);
    assert!(id.is_ok() && taxonomy_id.is_ok() && bag.is_some());
    if let (Ok(id), Ok(taxonomy_id), Some(mut terms)) = (id, taxonomy_id, bag) {
        let draft = Draft::new();
        let inserted = terms.insert(&draft);
        assert!(inserted.is_err());
        if let Err(error) = inserted {
            assert!(error.is_unavailable());
        }
        let updated = terms.update(id, taxonomy_id, &draft);
        assert!(updated.is_err());
        if let Err(error) = updated {
            assert!(error.is_unavailable());
        }
        let deleted = terms.delete(id, taxonomy_id);
        assert!(deleted.is_err());
        if let Err(error) = deleted {
            assert!(error.is_unavailable());
        }
    }
}
