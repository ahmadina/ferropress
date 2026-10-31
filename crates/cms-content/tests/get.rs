//! Post get by id (`contracts/content/get.md`). SQL text was captured from the 7.1.3 lab.

use cms_content::{PostId, Posts};
use cms_db::{Cell, Column, Reply, ResultSet, Row, Scripted, ServerError, Sql, TablePrefix};

fn get_sql(id: u64) -> Sql {
    Sql::from(format!("SELECT * FROM wp_posts WHERE ID = {id} LIMIT 1").as_str())
}

fn row(id: &str, title: &str, status: &str) -> Reply {
    Reply::Rows(ResultSet::new(
        [
            Column::new("ID"),
            Column::new("post_title"),
            Column::new("post_status"),
        ],
        [Row::new([
            Cell::bytes(id),
            Cell::bytes(title),
            Cell::bytes(status),
        ])],
    ))
}

#[test]
fn select_then_get_from_the_bag() {
    let store = Scripted::new([(get_sql(12), row("12", "Hello", "publish"))]);
    let prefix = TablePrefix::new("wp_");
    let id = PostId::new(12);
    assert!(prefix.is_ok() && id.is_ok());
    if let (Ok(prefix), Ok(id)) = (prefix, id) {
        let opened = Posts::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut posts) = opened {
            let first = posts.get(id);
            assert!(first.as_ref().is_ok_and(|post| {
                post.as_ref().is_some_and(|post| {
                    post.id() == id && post.title() == b"Hello" && post.status() == b"publish"
                })
            }));
            let second = posts.get(id);
            assert!(
                second.as_ref().is_ok_and(|post| {
                    post.as_ref().is_some_and(|post| post.title() == b"Hello")
                })
            );
            let shown = format!("{posts:?}");
            assert!(shown.contains("Posts"));
        }
    }
}

#[test]
fn a_miss_is_not_remembered() {
    let store = Scripted::new([
        (
            get_sql(99),
            Reply::Rows(ResultSet::new([Column::new("ID")], Vec::<Row>::new())),
        ),
        (get_sql(99), row("99", "Later", "draft")),
    ]);
    let prefix = TablePrefix::new("wp_");
    let id = PostId::new(99);
    assert!(prefix.is_ok() && id.is_ok());
    if let (Ok(prefix), Ok(id)) = (prefix, id) {
        let opened = Posts::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut posts) = opened {
            let first = posts.get(id);
            assert!(first.as_ref().is_ok_and(Option::is_none));
            let second = posts.get(id);
            assert!(
                second.as_ref().is_ok_and(|post| {
                    post.as_ref().is_some_and(|post| post.title() == b"Later")
                })
            );
        }
    }
}

#[test]
fn null_cells_become_empty_bytes() {
    let store = Scripted::new([(
        get_sql(3),
        Reply::Rows(ResultSet::new(
            [
                Column::new("ID"),
                Column::new("post_title"),
                Column::new("post_content"),
            ],
            [Row::new([Cell::bytes("3"), Cell::Null, Cell::Null])],
        )),
    )]);
    let prefix = TablePrefix::new("wp_");
    let id = PostId::new(3);
    assert!(prefix.is_ok() && id.is_ok());
    if let (Ok(prefix), Ok(id)) = (prefix, id) {
        let opened = Posts::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut posts) = opened {
            let value = posts.get(id);
            assert!(value.as_ref().is_ok_and(|post| {
                post.as_ref()
                    .is_some_and(|post| post.title().is_empty() && post.content().is_empty())
            }));
        }
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
                [Column::new("ID")],
                [Row::new([Cell::bytes("0")])],
            )),
        ),
    ]);
    let prefix = TablePrefix::new("wp_");
    let four = PostId::new(4);
    let five = PostId::new(5);
    let six = PostId::new(6);
    assert!(prefix.is_ok() && four.is_ok() && five.is_ok() && six.is_ok());
    if let (Ok(prefix), Ok(four), Ok(five), Ok(six)) = (prefix, four, five, six) {
        let opened = Posts::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut posts) = opened {
            assert!(posts.get(four).as_ref().is_ok_and(Option::is_none));
            assert!(posts.get(five).as_ref().is_ok_and(Option::is_none));
            assert!(posts.get(six).as_ref().is_ok_and(Option::is_none));
        }
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let store = Scripted::new([]);
    let prefix = TablePrefix::new("wp_");
    let id = PostId::new(1);
    assert!(prefix.is_ok() && id.is_ok());
    if let (Ok(prefix), Ok(id)) = (prefix, id) {
        let opened = Posts::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut posts) = opened {
            let value = posts.get(id);
            assert!(value.is_err());
            if let Err(error) = value {
                assert!(error.is_unavailable());
            }
        }
    }
}
