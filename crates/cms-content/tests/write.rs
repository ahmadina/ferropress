//! Post insert, update and delete (`contracts/content/write.md`). SQL text was captured from the 7.1.3 lab.

use cms_content::{Draft, Field, PostId, Posts, Write};
use cms_db::{Cell, Column, Reply, ResultSet, Row, Scripted, ServerError, Sql, TablePrefix};

fn get_sql(id: u64) -> Sql {
    Sql::from(format!("SELECT * FROM wp_posts WHERE ID = {id} LIMIT 1").as_str())
}

fn insert_sql() -> Sql {
    Sql::from(
        "INSERT INTO `wp_posts` (`post_author`, `post_date`, `post_date_gmt`, `post_content`, `post_content_filtered`, `post_title`, `post_excerpt`, `post_status`, `post_type`, `comment_status`, `ping_status`, `post_password`, `post_name`, `to_ping`, `pinged`, `post_modified`, `post_modified_gmt`, `post_parent`, `menu_order`, `post_mime_type`, `guid`) VALUES (1, '2026-01-01 00:00:00', '2026-01-01 00:00:00', 'body', '', 'Hello', '', 'publish', 'post', 'open', 'open', '', 'hello', '', '', '2026-01-01 00:00:00', '2026-01-01 00:00:00', 0, 0, '', '')",
    )
}

fn update_sql(id: u64) -> Sql {
    Sql::from(
        format!(
            "UPDATE `wp_posts` SET `post_author` = 1, `post_date` = '2026-01-01 00:00:00', `post_date_gmt` = '2026-01-01 00:00:00', `post_content` = 'body', `post_content_filtered` = '', `post_title` = 'Hello', `post_excerpt` = '', `post_status` = 'publish', `post_type` = 'post', `comment_status` = 'open', `ping_status` = 'open', `post_password` = '', `post_name` = 'hello', `to_ping` = '', `pinged` = '', `post_modified` = '2026-01-01 00:00:00', `post_modified_gmt` = '2026-01-01 00:00:00', `post_parent` = 0, `menu_order` = 0, `post_mime_type` = '', `guid` = '' WHERE `ID` = {id}"
        )
        .as_str(),
    )
}

fn delete_sql(id: u64) -> Sql {
    Sql::from(format!("DELETE FROM `wp_posts` WHERE `ID` = {id}").as_str())
}

fn row(id: &str, title: &str) -> Reply {
    Reply::Rows(ResultSet::new(
        [Column::new("ID"), Column::new("post_title")],
        [Row::new([Cell::bytes(id), Cell::bytes(title)])],
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
        .set(Field::Author, b"1")
        .set(Field::Date, b"2026-01-01 00:00:00")
        .set(Field::DateGmt, b"2026-01-01 00:00:00")
        .set(Field::Content, b"body")
        .set(Field::Title, b"Hello")
        .set(Field::Status, b"publish")
        .set(Field::TypeName, b"post")
        .set(Field::CommentStatus, b"open")
        .set(Field::PingStatus, b"open")
        .set(Field::Name, b"hello")
        .set(Field::Modified, b"2026-01-01 00:00:00")
        .set(Field::ModifiedGmt, b"2026-01-01 00:00:00");
    draft
}

fn opened(store: Scripted) -> Option<Posts<Scripted>> {
    let prefix = TablePrefix::new("wp_");
    if let Ok(prefix) = prefix {
        Posts::new(store, &prefix).ok()
    } else {
        None
    }
}

#[test]
fn insert_returns_the_new_id() {
    let store = Scripted::new([(insert_sql(), done(1, 42))]);
    let bag = opened(store);
    assert!(bag.is_some());
    if let Some(mut posts) = bag {
        let written = posts.insert(&sample_draft());
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
        (get_sql(12), row("12", "Hello")),
    ]);
    let id = PostId::new(12);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut posts)) = (id, bag) {
        let first = posts.get(id);
        assert!(
            first
                .as_ref()
                .is_ok_and(|post| { post.as_ref().is_some_and(|post| post.title() == b"Old") })
        );
        let written = posts.update(id, &sample_draft());
        assert!(
            written
                .as_ref()
                .is_ok_and(|write| *write == Write::Replaced)
        );
        let second = posts.get(id);
        assert!(
            second
                .as_ref()
                .is_ok_and(|post| { post.as_ref().is_some_and(|post| post.title() == b"Hello") })
        );
    }
}

#[test]
fn delete_removes_and_forgets_a_hit() {
    let store = Scripted::new([
        (get_sql(12), row("12", "Hello")),
        (delete_sql(12), done(1, 0)),
        (
            get_sql(12),
            Reply::Rows(ResultSet::new([Column::new("ID")], Vec::<Row>::new())),
        ),
    ]);
    let id = PostId::new(12);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut posts)) = (id, bag) {
        assert!(posts.get(id).as_ref().is_ok_and(Option::is_some));
        let written = posts.delete(id);
        assert!(written.as_ref().is_ok_and(|write| *write == Write::Removed));
        assert!(posts.get(id).as_ref().is_ok_and(Option::is_none));
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
    let id = PostId::new(12);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut posts)) = (id, bag) {
        let draft = sample_draft();
        assert!(
            posts
                .insert(&draft)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            posts
                .insert(&draft)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            posts
                .insert(&draft)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            posts
                .update(id, &draft)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            posts
                .update(id, &draft)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            posts
                .delete(id)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
        assert!(
            posts
                .delete(id)
                .as_ref()
                .is_ok_and(|write| *write == Write::Unchanged)
        );
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let store = Scripted::new([]);
    let id = PostId::new(1);
    let bag = opened(store);
    assert!(id.is_ok() && bag.is_some());
    if let (Ok(id), Some(mut posts)) = (id, bag) {
        let draft = Draft::new();
        let inserted = posts.insert(&draft);
        assert!(inserted.is_err());
        if let Err(error) = inserted {
            assert!(error.is_unavailable());
        }
        let updated = posts.update(id, &draft);
        assert!(updated.is_err());
        if let Err(error) = updated {
            assert!(error.is_unavailable());
        }
        let deleted = posts.delete(id);
        assert!(deleted.is_err());
        if let Err(error) = deleted {
            assert!(error.is_unavailable());
        }
    }
}
