//! Comment insert, update and delete (`contracts/comments/write.md`). SQL text was captured from the 7.1.3 lab.

use cms_comments::{CommentId, Comments, Draft, Field, Write};
use cms_db::{Cell, Column, Reply, ResultSet, Row, Scripted, ServerError, Sql, TablePrefix};

fn get_sql(id: u64) -> Sql {
    Sql::from(format!("SELECT * FROM wp_comments WHERE comment_ID = {id} LIMIT 1").as_str())
}

fn insert_sql() -> Sql {
    Sql::from(
        "INSERT INTO `wp_comments` (`comment_post_ID`, `comment_author_IP`, `comment_author`, `comment_author_email`, `comment_author_url`, `comment_date`, `comment_date_gmt`, `comment_content`, `comment_karma`, `comment_approved`, `comment_agent`, `comment_type`, `comment_parent`, `user_id`) VALUES (3, '203.0.113.4', 'Ada', 'ada@example.test', 'https://ada.test', '2026-01-01 00:00:00', '2026-01-01 00:00:00', 'hello', 0, '1', 'agent', 'comment', 0, 2)",
    )
}

fn update_sql(id: u64) -> Sql {
    Sql::from(
        format!(
            "UPDATE `wp_comments` SET `comment_post_ID` = 3, `comment_author` = 'Ada', `comment_author_email` = 'ada@example.test', `comment_author_url` = 'https://ada.test', `comment_author_IP` = '203.0.113.4', `comment_date` = '2026-01-01 00:00:00', `comment_date_gmt` = '2026-01-01 00:00:00', `comment_content` = 'hello', `comment_karma` = 0, `comment_approved` = '1', `comment_agent` = 'agent', `comment_type` = 'comment', `comment_parent` = 0, `user_id` = 2 WHERE `comment_ID` = {id}"
        )
        .as_str(),
    )
}

fn delete_sql(id: u64) -> Sql {
    Sql::from(format!("DELETE FROM `wp_comments` WHERE `comment_ID` = {id}").as_str())
}

fn row(id: &str, content: &str) -> Reply {
    Reply::Rows(ResultSet::new(
        [Column::new("comment_ID"), Column::new("comment_content")],
        [Row::new([Cell::bytes(id), Cell::bytes(content)])],
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
        .set(Field::Post, b"3")
        .set(Field::AuthorIp, b"203.0.113.4")
        .set(Field::Author, b"Ada")
        .set(Field::AuthorEmail, b"ada@example.test")
        .set(Field::AuthorUrl, b"https://ada.test")
        .set(Field::Date, b"2026-01-01 00:00:00")
        .set(Field::DateGmt, b"2026-01-01 00:00:00")
        .set(Field::Content, b"hello")
        .set(Field::Karma, b"0")
        .set(Field::Approved, b"1")
        .set(Field::Agent, b"agent")
        .set(Field::TypeName, b"comment")
        .set(Field::Parent, b"0")
        .set(Field::UserId, b"2");
    draft
}

fn opened(store: Scripted) -> Option<Comments<Scripted>> {
    let prefix = TablePrefix::new("wp_");
    if let Ok(prefix) = prefix {
        Comments::new(store, &prefix).ok()
    } else {
        None
    }
}

#[test]
fn insert_returns_the_new_id() {
    let store = Scripted::new([(insert_sql(), done(1, 42))]);
    let bag = opened(store);
    assert!(bag.is_some());
    if let Some(mut comments) = bag {
        let written = comments.insert(&sample_draft());
        assert!(written.is_ok());
        if let Ok(written) = written {
            assert!(written.changed());
            let id = written.inserted_id();
            assert!(id.is_some());
            if let Some(id) = id {
                assert_eq!(id.as_u64(), 42);
            }
        }
    }
}

#[test]
fn update_rewrites_and_forgets_a_hit() {
    let store = Scripted::new([
        (get_sql(12), row("12", "Old")),
        (update_sql(12), done(1, 0)),
        (get_sql(12), row("12", "hello")),
    ]);
    let id = CommentId::new(12);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut comments)) = (id, bag) {
        let first = comments.get(id);
        assert!(first.as_ref().is_ok_and(|comment| {
            comment
                .as_ref()
                .is_some_and(|comment| comment.content() == b"Old")
        }));
        let written = comments.update(id, &sample_draft());
        assert!(
            written
                .as_ref()
                .is_ok_and(|write| *write == Write::Replaced)
        );
        let second = comments.get(id);
        assert!(second.as_ref().is_ok_and(|comment| {
            comment
                .as_ref()
                .is_some_and(|comment| comment.content() == b"hello")
        }));
    }
}

#[test]
fn delete_removes_and_forgets_a_hit() {
    let store = Scripted::new([
        (get_sql(12), row("12", "hello")),
        (delete_sql(12), done(1, 0)),
        (
            get_sql(12),
            Reply::Rows(ResultSet::new(
                [Column::new("comment_ID")],
                Vec::<Row>::new(),
            )),
        ),
    ]);
    let id = CommentId::new(12);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut comments)) = (id, bag) {
        assert!(comments.get(id).as_ref().is_ok_and(Option::is_some));
        let written = comments.delete(id);
        assert!(written.as_ref().is_ok_and(|write| *write == Write::Removed));
        assert!(comments.get(id).as_ref().is_ok_and(Option::is_none));
    }
}

#[test]
fn failed_or_zero_is_unchanged() {
    let store = Scripted::new([
        (insert_sql(), done(1, 0)),
        (insert_sql(), failed()),
        (insert_sql(), row("1", "x")),
        (update_sql(12), done(0, 0)),
        (update_sql(12), failed()),
        (delete_sql(12), done(0, 0)),
        (delete_sql(12), failed()),
    ]);
    let id = CommentId::new(12);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut comments)) = (id, bag) {
        let draft = sample_draft();
        assert!(
            comments
                .insert(&draft)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            comments
                .insert(&draft)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            comments
                .insert(&draft)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            comments
                .update(id, &draft)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            comments
                .update(id, &draft)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            comments
                .delete(id)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            comments
                .delete(id)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let store = Scripted::new([]);
    let id = CommentId::new(1);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut comments)) = (id, bag) {
        let draft = Draft::new();
        let inserted = comments.insert(&draft);
        assert!(inserted.is_err());
        if let Err(error) = inserted {
            assert!(error.is_unavailable());
        }
        let updated = comments.update(id, &draft);
        assert!(updated.is_err());
        if let Err(error) = updated {
            assert!(error.is_unavailable());
        }
        let deleted = comments.delete(id);
        assert!(deleted.is_err());
        if let Err(error) = deleted {
            assert!(error.is_unavailable());
        }
    }
}
