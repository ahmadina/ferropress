//! Term get by id (`contracts/taxonomy/get.md`). SQL text was captured from the 7.1.3 lab.

use cms_db::{Cell, Column, Reply, ResultSet, Row, Scripted, ServerError, Sql, TablePrefix};
use cms_taxonomy::{Found, TermId, Terms};

fn get_sql(id: u64) -> Sql {
    Sql::from(
        format!(
            "SELECT t.*, tt.* FROM wp_terms AS t INNER JOIN wp_term_taxonomy AS tt ON t.term_id = tt.term_id WHERE t.term_id = {id}"
        )
        .as_str(),
    )
}

fn columns() -> [Column; 4] {
    [
        Column::new("term_id"),
        Column::new("name"),
        Column::new("taxonomy"),
        Column::new("slug"),
    ]
}

fn row(id: &str, name: &str, taxonomy: &str, slug: &str) -> Row {
    Row::new([
        Cell::bytes(id),
        Cell::bytes(name),
        Cell::bytes(taxonomy),
        Cell::bytes(slug),
    ])
}

fn rows(items: impl Into<Vec<Row>>) -> Reply {
    Reply::Rows(ResultSet::new(columns(), items))
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
fn select_then_get_from_the_bag() {
    let store = Scripted::new([(get_sql(12), rows([row("12", "News", "category", "news")]))]);
    let id = TermId::new(12);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut terms)) = (id, bag) {
        let first = terms.get(id, b"");
        assert!(first.as_ref().is_ok_and(|found| {
            found.term().is_some_and(|term| {
                term.id() == id && term.name() == b"News" && term.taxonomy() == b"category"
            })
        }));
        let second = terms.get(id, b"category");
        assert!(
            second
                .as_ref()
                .is_ok_and(|found| found.term().is_some_and(|term| term.slug() == b"news"))
        );
        let shown = format!("{terms:?}");
        assert!(shown.contains("Terms"));
    }
}

#[test]
fn a_taxonomy_filter_picks_the_matching_row() {
    let shared = rows([
        row("4", "News", "category", "news"),
        row("4", "News", "post_tag", "news"),
    ]);
    let store = Scripted::new([(get_sql(4), shared.clone()), (get_sql(4), shared)]);
    let id = TermId::new(4);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut terms)) = (id, bag) {
        let found = terms.get(id, b"post_tag");
        assert!(found.as_ref().is_ok_and(|found| {
            found
                .term()
                .is_some_and(|term| term.taxonomy() == b"post_tag")
        }));
        let again = terms.get(id, b"category");
        assert!(again.as_ref().is_ok_and(|found| {
            found
                .term()
                .is_some_and(|term| term.taxonomy() == b"category")
        }));
    }
}

#[test]
fn a_shared_id_is_not_remembered() {
    let store = Scripted::new([
        (
            get_sql(4),
            rows([
                row("4", "News", "category", "news"),
                row("4", "News", "post_tag", "news"),
            ]),
        ),
        (get_sql(4), rows([row("4", "News", "category", "news")])),
    ]);
    let id = TermId::new(4);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut terms)) = (id, bag) {
        let first = terms.get(id, b"");
        assert!(first.as_ref().is_ok_and(Found::is_shared));
        let second = terms.get(id, b"");
        assert!(second.as_ref().is_ok_and(|found| {
            found
                .term()
                .is_some_and(|term| term.taxonomy() == b"category")
        }));
    }
}

#[test]
fn a_cached_row_is_ignored_for_another_taxonomy() {
    let store = Scripted::new([
        (get_sql(8), rows([row("8", "News", "category", "news")])),
        (
            get_sql(8),
            rows([
                row("8", "News", "category", "news"),
                row("8", "News", "post_tag", "news"),
            ]),
        ),
    ]);
    let id = TermId::new(8);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut terms)) = (id, bag) {
        assert!(terms.get(id, b"").as_ref().is_ok_and(|found| {
            found
                .term()
                .is_some_and(|term| term.taxonomy() == b"category")
        }));
        let filtered = terms.get(id, b"post_tag");
        assert!(filtered.as_ref().is_ok_and(|found| {
            found
                .term()
                .is_some_and(|term| term.taxonomy() == b"post_tag")
        }));
    }
}

#[test]
fn a_miss_is_not_remembered() {
    let store = Scripted::new([
        (
            get_sql(99),
            Reply::Rows(ResultSet::new([Column::new("term_id")], Vec::<Row>::new())),
        ),
        (get_sql(99), rows([row("99", "Later", "category", "later")])),
    ]);
    let id = TermId::new(99);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut terms)) = (id, bag) {
        assert!(terms.get(id, b"").as_ref().is_ok_and(Found::is_miss));
        let second = terms.get(id, b"");
        assert!(
            second
                .as_ref()
                .is_ok_and(|found| found.term().is_some_and(|term| term.name() == b"Later"))
        );
    }
}

#[test]
fn a_done_or_failed_get_is_a_miss() {
    let store = Scripted::new([
        (
            get_sql(4),
            Reply::Done {
                affected: 0,
                insert_id: 0,
            },
        ),
        (get_sql(5), Reply::Failed(ServerError::new(1146, "missing"))),
        (
            get_sql(6),
            Reply::Rows(ResultSet::new(
                [Column::new("term_id")],
                [Row::new([Cell::bytes("0")])],
            )),
        ),
        (get_sql(7), rows([row("7", "News", "category", "news")])),
    ]);
    let four = TermId::new(4);
    let five = TermId::new(5);
    let six = TermId::new(6);
    let seven = TermId::new(7);
    let bag = opened(store);
    assert!(four.is_ok() && five.is_ok() && six.is_ok() && seven.is_ok() && bag.is_some());
    if let (Ok(four), Ok(five), Ok(six), Ok(seven), Some(mut terms)) = (four, five, six, seven, bag)
    {
        assert!(terms.get(four, b"").as_ref().is_ok_and(Found::is_miss));
        assert!(terms.get(five, b"").as_ref().is_ok_and(Found::is_miss));
        assert!(terms.get(six, b"").as_ref().is_ok_and(Found::is_miss));
        assert!(
            terms
                .get(seven, b"post_tag")
                .as_ref()
                .is_ok_and(Found::is_miss)
        );
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let store = Scripted::new([]);
    let id = TermId::new(1);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut terms)) = (id, bag) {
        let value = terms.get(id, b"");
        assert!(value.is_err());
        if let Err(error) = value {
            assert!(error.is_unavailable());
        }
    }
}
