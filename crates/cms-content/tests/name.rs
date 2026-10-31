//! Post get by name (`contracts/content/name.md`).

use cms_content::Posts;
use cms_db::{Cell, Column, Reply, ResultSet, Row, Scripted, Sql, TablePrefix};

fn name_sql(slug: &str) -> Sql {
    Sql::from(format!("SELECT * FROM wp_posts WHERE post_name = '{slug}' LIMIT 1").as_str())
}

fn row(id: &str, title: &str) -> Reply {
    Reply::Rows(ResultSet::new(
        [Column::new("ID"), Column::new("post_title")],
        [Row::new([Cell::bytes(id), Cell::bytes(title)])],
    ))
}

fn open(store: Scripted) -> Option<Posts<Scripted>> {
    let prefix = TablePrefix::new("wp_");
    prefix
        .ok()
        .and_then(|prefix| Posts::new(store, &prefix).ok())
}

#[test]
fn hello_loads_the_row() {
    let store = Scripted::new([(name_sql("hello"), row("12", "Hello"))]);
    let opened = open(store);
    assert!(opened.is_some());
    if let Some(mut posts) = opened {
        let loaded = posts.by_name(b"hello");
        assert!(loaded.as_ref().is_ok_and(|post| {
            post.as_ref()
                .is_some_and(|post| post.id().as_u64() == 12 && post.title() == b"Hello")
        }));
    }
}

#[test]
fn title_case_is_sanitized() {
    let store = Scripted::new([(name_sql("hello-world"), row("12", "Hello"))]);
    let opened = open(store);
    assert!(opened.is_some());
    if let Some(mut posts) = opened {
        let loaded = posts.by_name(b"Hello World");
        assert!(
            loaded
                .as_ref()
                .is_ok_and(|post| { post.as_ref().is_some_and(|post| post.id().as_u64() == 12) })
        );
    }
}

#[test]
fn a_miss_is_empty() {
    let store = Scripted::new([(
        name_sql("missing"),
        Reply::Rows(ResultSet::new([Column::new("ID")], Vec::<Row>::new())),
    )]);
    let opened = open(store);
    assert!(opened.is_some());
    if let Some(mut posts) = opened {
        let loaded = posts.by_name(b"missing");
        assert!(loaded.as_ref().is_ok_and(Option::is_none));
    }
}

#[test]
fn a_failed_name_query_is_a_miss() {
    let store = Scripted::new([(
        name_sql("hello"),
        Reply::Failed(cms_db::ServerError::new(1146, "missing")),
    )]);
    let opened = open(store);
    assert!(opened.is_some());
    if let Some(mut posts) = opened {
        let loaded = posts.by_name(b"hello");
        assert!(loaded.as_ref().is_ok_and(Option::is_none));
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let store = Scripted::new([]);
    let opened = open(store);
    assert!(opened.is_some());
    if let Some(mut posts) = opened {
        let loaded = posts.by_name(b"hello");
        assert!(loaded.is_err_and(|error| error.is_unavailable()));
    }
}
