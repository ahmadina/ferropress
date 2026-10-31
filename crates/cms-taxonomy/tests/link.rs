//! Object–term relationships (`contracts/taxonomy/link.md`). SQL text was captured from the 7.1.3 lab.

use cms_db::{Cell, Column, Reply, ResultSet, Row, Scripted, ServerError, Sql, TablePrefix};
use cms_taxonomy::{Link, ObjectId, TermTaxonomyId, Terms};

fn exists_sql(object: u64, taxonomy_id: u64) -> Sql {
    Sql::from(
        format!(
            "SELECT term_taxonomy_id FROM wp_term_relationships WHERE object_id = {object} AND term_taxonomy_id = {taxonomy_id}"
        )
        .as_str(),
    )
}

fn objects_sql(taxonomy_id: u64) -> Sql {
    Sql::from(
        format!(
            "SELECT object_id FROM wp_term_relationships WHERE term_taxonomy_id = {taxonomy_id}"
        )
        .as_str(),
    )
}

fn insert_sql(object: u64, taxonomy_id: u64) -> Sql {
    Sql::from(
        format!(
            "INSERT INTO `wp_term_relationships` (`object_id`, `term_taxonomy_id`) VALUES ({object}, {taxonomy_id})"
        )
        .as_str(),
    )
}

fn delete_sql(object: u64, ids: &str) -> Sql {
    Sql::from(
        format!(
            "DELETE FROM wp_term_relationships WHERE object_id = {object} AND term_taxonomy_id IN ({ids})"
        )
        .as_str(),
    )
}

fn tt_row(id: &str) -> Reply {
    Reply::Rows(ResultSet::new(
        [Column::new("term_taxonomy_id")],
        [Row::new([Cell::bytes(id)])],
    ))
}

fn empty() -> Reply {
    Reply::Rows(ResultSet::new(
        [Column::new("term_taxonomy_id")],
        Vec::<Row>::new(),
    ))
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

fn opened(store: Scripted) -> Option<Terms<Scripted>> {
    let prefix = TablePrefix::new("wp_");
    if let Ok(prefix) = prefix {
        Terms::new(store, &prefix).ok()
    } else {
        None
    }
}

#[test]
fn attach_inserts_a_missing_pair() {
    let store = Scripted::new([(exists_sql(7, 9), empty()), (insert_sql(7, 9), done(1))]);
    let object = ObjectId::new(7);
    let taxonomy_id = TermTaxonomyId::new(9);
    let bag = opened(store);
    assert!(object.is_ok() && taxonomy_id.is_ok() && bag.is_some());
    if let (Ok(object), Ok(taxonomy_id), Some(mut terms)) = (object, taxonomy_id, bag) {
        let written = terms.attach(object, taxonomy_id);
        assert!(written.as_ref().is_ok_and(|link| *link == Link::Attached));
        let shown = format!("{terms:?}");
        assert!(shown.contains("Terms"));
    }
}

#[test]
fn attach_skips_an_existing_pair() {
    let store = Scripted::new([
        (exists_sql(7, 9), tt_row("9")),
        (exists_sql(7, 9), tt_row("9")),
    ]);
    let object = ObjectId::new(7);
    let taxonomy_id = TermTaxonomyId::new(9);
    let bag = opened(store);
    assert!(object.is_ok() && taxonomy_id.is_ok() && bag.is_some());
    if let (Ok(object), Ok(taxonomy_id), Some(mut terms)) = (object, taxonomy_id, bag) {
        let written = terms.attach(object, taxonomy_id);
        assert!(written.as_ref().is_ok_and(|link| *link == Link::Unchanged));
        assert!(
            terms
                .linked(object, taxonomy_id)
                .as_ref()
                .is_ok_and(|hit| *hit)
        );
    }
}

#[test]
fn detach_removes_quoted_ids() {
    let nine = TermTaxonomyId::new(9);
    let twelve = TermTaxonomyId::new(12);
    let object = ObjectId::new(7);
    let store = Scripted::new([(delete_sql(7, "'9', '12'"), done(2))]);
    let bag = opened(store);
    assert!(nine.is_ok() && twelve.is_ok() && object.is_ok() && bag.is_some());
    if let (Ok(nine), Ok(twelve), Ok(object), Some(mut terms)) = (nine, twelve, object, bag) {
        let written = terms.detach(object, &[nine, twelve]);
        assert!(written.as_ref().is_ok_and(|link| *link == Link::Detached));
        assert!(
            terms
                .detach(object, &[])
                .as_ref()
                .is_ok_and(|link| *link == Link::Unchanged)
        );
    }
}

#[test]
fn objects_lists_usable_ids() {
    let store = Scripted::new([(
        objects_sql(9),
        Reply::Rows(ResultSet::new(
            [Column::new("object_id")],
            [
                Row::new([Cell::bytes("7")]),
                Row::new([Cell::Null]),
                Row::new([Cell::bytes("0")]),
                Row::new([Cell::bytes("x")]),
                Row::new(Vec::<Cell>::new()),
                Row::new([Cell::bytes("8")]),
            ],
        )),
    )]);
    let taxonomy_id = TermTaxonomyId::new(9);
    let bag = opened(store);
    assert!(taxonomy_id.is_ok() && bag.is_some());
    if let (Ok(taxonomy_id), Some(mut terms)) = (taxonomy_id, bag) {
        let listed = terms.objects(taxonomy_id);
        assert!(listed.is_ok());
        if let Ok(listed) = listed {
            assert_eq!(listed.len(), 2);
            assert_eq!(listed[0].as_u64(), 7);
            assert_eq!(listed[1].as_u64(), 8);
        }
    }
}

#[test]
fn failed_or_zero_is_unchanged() {
    let store = Scripted::new([
        (exists_sql(7, 9), failed()),
        (insert_sql(7, 9), done(0)),
        (exists_sql(7, 9), empty()),
        (insert_sql(7, 9), failed()),
        (delete_sql(7, "'9'"), done(0)),
        (objects_sql(9), failed()),
        (exists_sql(7, 9), tt_row("0")),
    ]);
    let object = ObjectId::new(7);
    let taxonomy_id = TermTaxonomyId::new(9);
    let bag = opened(store);
    assert!(object.is_ok() && taxonomy_id.is_ok() && bag.is_some());
    if let (Ok(object), Ok(taxonomy_id), Some(mut terms)) = (object, taxonomy_id, bag) {
        assert!(
            terms
                .attach(object, taxonomy_id)
                .as_ref()
                .is_ok_and(|link| *link == Link::Unchanged)
        );
        assert!(
            terms
                .attach(object, taxonomy_id)
                .as_ref()
                .is_ok_and(|link| *link == Link::Unchanged)
        );
        assert!(
            terms
                .detach(object, &[taxonomy_id])
                .as_ref()
                .is_ok_and(|link| *link == Link::Unchanged)
        );
        let listed = terms.objects(taxonomy_id);
        assert!(listed.as_ref().is_ok_and(Vec::is_empty));
        assert!(
            terms
                .linked(object, taxonomy_id)
                .as_ref()
                .is_ok_and(|hit| !hit)
        );
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let store = Scripted::new([]);
    let object = ObjectId::new(1);
    let taxonomy_id = TermTaxonomyId::new(2);
    let bag = opened(store);
    assert!(object.is_ok() && taxonomy_id.is_ok() && bag.is_some());
    if let (Ok(object), Ok(taxonomy_id), Some(mut terms)) = (object, taxonomy_id, bag) {
        let linked = terms.linked(object, taxonomy_id);
        assert!(linked.is_err());
        if let Err(error) = linked {
            assert!(error.is_unavailable());
        }
        let attached = terms.attach(object, taxonomy_id);
        assert!(attached.is_err());
        if let Err(error) = attached {
            assert!(error.is_unavailable());
        }
        let detached = terms.detach(object, &[taxonomy_id]);
        assert!(detached.is_err());
        if let Err(error) = detached {
            assert!(error.is_unavailable());
        }
        let listed = terms.objects(taxonomy_id);
        assert!(listed.is_err());
        if let Err(error) = listed {
            assert!(error.is_unavailable());
        }
    }
}
