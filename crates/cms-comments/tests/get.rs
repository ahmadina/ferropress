//! Comment get by id (`contracts/comments/get.md`). SQL text was captured from the 7.1.3 lab.

use cms_comments::{CommentId, Comments};
use cms_db::{Cell, Column, Reply, ResultSet, Row, Scripted, ServerError, Sql, TablePrefix};

fn get_sql(id: u64) -> Sql {
    Sql::from(format!("SELECT * FROM wp_comments WHERE comment_ID = {id} LIMIT 1").as_str())
}

fn row(id: &str, content: &str, approved: &str) -> Reply {
    Reply::Rows(ResultSet::new(
        [
            Column::new("comment_ID"),
            Column::new("comment_content"),
            Column::new("comment_approved"),
        ],
        [Row::new([
            Cell::bytes(id),
            Cell::bytes(content),
            Cell::bytes(approved),
        ])],
    ))
}

#[test]
fn select_then_get_from_the_bag() {
    let store = Scripted::new([(get_sql(12), row("12", "Hello", "1"))]);
    let prefix = TablePrefix::new("wp_");
    let id = CommentId::new(12);
    assert!(prefix.is_ok() && id.is_ok());
    if let (Ok(prefix), Ok(id)) = (prefix, id) {
        let opened = Comments::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut comments) = opened {
            let first = comments.get(id);
            assert!(first.as_ref().is_ok_and(|comment| {
                comment.as_ref().is_some_and(|comment| {
                    comment.id() == id
                        && comment.content() == b"Hello"
                        && comment.approved() == b"1"
                })
            }));
            let second = comments.get(id);
            assert!(second.as_ref().is_ok_and(|comment| {
                comment
                    .as_ref()
                    .is_some_and(|comment| comment.content() == b"Hello")
            }));
            let shown = format!("{comments:?}");
            assert!(shown.contains("Comments"));
        }
    }
}

#[test]
fn a_miss_is_not_remembered() {
    let store = Scripted::new([
        (
            get_sql(99),
            Reply::Rows(ResultSet::new(
                [Column::new("comment_ID")],
                Vec::<Row>::new(),
            )),
        ),
        (get_sql(99), row("99", "Later", "0")),
    ]);
    let prefix = TablePrefix::new("wp_");
    let id = CommentId::new(99);
    assert!(prefix.is_ok() && id.is_ok());
    if let (Ok(prefix), Ok(id)) = (prefix, id) {
        let opened = Comments::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut comments) = opened {
            let first = comments.get(id);
            assert!(first.as_ref().is_ok_and(Option::is_none));
            let second = comments.get(id);
            assert!(second.as_ref().is_ok_and(|comment| {
                comment
                    .as_ref()
                    .is_some_and(|comment| comment.content() == b"Later")
            }));
        }
    }
}

#[test]
fn null_cells_become_empty_bytes() {
    let store = Scripted::new([(
        get_sql(3),
        Reply::Rows(ResultSet::new(
            [
                Column::new("comment_ID"),
                Column::new("comment_author"),
                Column::new("comment_content"),
            ],
            [Row::new([Cell::bytes("3"), Cell::Null, Cell::Null])],
        )),
    )]);
    let prefix = TablePrefix::new("wp_");
    let id = CommentId::new(3);
    assert!(prefix.is_ok() && id.is_ok());
    if let (Ok(prefix), Ok(id)) = (prefix, id) {
        let opened = Comments::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut comments) = opened {
            let value = comments.get(id);
            assert!(value.as_ref().is_ok_and(|comment| {
                comment.as_ref().is_some_and(|comment| {
                    comment.author().is_empty() && comment.content().is_empty()
                })
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
                [Column::new("comment_ID")],
                [Row::new([Cell::bytes("0")])],
            )),
        ),
    ]);
    let prefix = TablePrefix::new("wp_");
    let four = CommentId::new(4);
    let five = CommentId::new(5);
    let six = CommentId::new(6);
    assert!(prefix.is_ok() && four.is_ok() && five.is_ok() && six.is_ok());
    if let (Ok(prefix), Ok(four), Ok(five), Ok(six)) = (prefix, four, five, six) {
        let opened = Comments::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut comments) = opened {
            assert!(comments.get(four).as_ref().is_ok_and(Option::is_none));
            assert!(comments.get(five).as_ref().is_ok_and(Option::is_none));
            assert!(comments.get(six).as_ref().is_ok_and(Option::is_none));
        }
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let store = Scripted::new([]);
    let prefix = TablePrefix::new("wp_");
    let id = CommentId::new(1);
    assert!(prefix.is_ok() && id.is_ok());
    if let (Ok(prefix), Ok(id)) = (prefix, id) {
        let opened = Comments::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut comments) = opened {
            let value = comments.get(id);
            assert!(value.is_err());
            if let Err(error) = value {
                assert!(error.is_unavailable());
            }
        }
    }
}
