//! Load the post row for a singular id (`contracts/query/load-id.md`).

use cms_db::{Cell, Column, Reply, ResultSet, Row, Scripted, Sql, TablePrefix};
use cms_query::{
    AttachmentInput, AttachmentVar, NameVar, PageNameVar, PageVar, PostVar, Query, Singular,
};

fn get_sql(id: u64) -> Sql {
    Sql::from(format!("SELECT * FROM wp_posts WHERE ID = {id} LIMIT 1").as_str())
}

fn row(id: &str, title: &str) -> Reply {
    Reply::Rows(ResultSet::new(
        [Column::new("ID"), Column::new("post_title")],
        [Row::new([Cell::bytes(id), Cell::bytes(title)])],
    ))
}

fn flags(
    attachment: &AttachmentVar,
    name: Option<&[u8]>,
    post: PostVar,
    page: PageVar,
) -> Singular {
    Singular::resolve(
        attachment,
        &NameVar::parse(name),
        post,
        &PageNameVar::parse(None),
        page,
    )
}

#[test]
fn p_loads_the_row() {
    let store = Scripted::new([(get_sql(12), row("12", "Hello"))]);
    let prefix = TablePrefix::new("wp_");
    assert!(prefix.is_ok());
    if let Ok(prefix) = prefix {
        let opened = Query::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut query) = opened {
            let attach = AttachmentVar::parse(AttachmentInput::new());
            let post = PostVar::parse(Some(b"12"));
            let page = PageVar::parse(None);
            let loaded = query.load_id(flags(&attach, None, post, page), post, page, &attach);
            assert!(loaded.as_ref().is_ok_and(|rows| {
                rows.len() == 1 && rows[0].id().as_u64() == 12 && rows[0].title() == b"Hello"
            }));
        }
    }
}

#[test]
fn page_id_loads_when_p_is_empty() {
    let store = Scripted::new([(get_sql(4), row("4", "About"))]);
    let prefix = TablePrefix::new("wp_");
    assert!(prefix.is_ok());
    if let Ok(prefix) = prefix {
        let opened = Query::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut query) = opened {
            let attach = AttachmentVar::parse(AttachmentInput::new());
            let post = PostVar::parse(None);
            let page = PageVar::parse(Some(b"4"));
            let loaded = query.load_id(flags(&attach, None, post, page), post, page, &attach);
            assert!(loaded.as_ref().is_ok_and(|rows| {
                rows.len() == 1 && rows[0].id().as_u64() == 4 && rows[0].title() == b"About"
            }));
        }
    }
}

#[test]
fn attachment_id_wins() {
    let store = Scripted::new([(get_sql(9), row("9", "File"))]);
    let prefix = TablePrefix::new("wp_");
    assert!(prefix.is_ok());
    if let Ok(prefix) = prefix {
        let opened = Query::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut query) = opened {
            let attach = AttachmentVar::parse(AttachmentInput::new().attachment_id(b"9"));
            let post = PostVar::parse(Some(b"12"));
            let page = PageVar::parse(Some(b"4"));
            let loaded = query.load_id(flags(&attach, None, post, page), post, page, &attach);
            assert!(loaded.as_ref().is_ok_and(|rows| rows.len() == 1
                && rows[0].id().as_u64() == 9
                && rows[0].title() == b"File"));
        }
    }
}

#[test]
fn name_only_issues_no_sql() {
    let store = Scripted::new([]);
    let prefix = TablePrefix::new("wp_");
    assert!(prefix.is_ok());
    if let Ok(prefix) = prefix {
        let opened = Query::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut query) = opened {
            let attach = AttachmentVar::parse(AttachmentInput::new());
            let post = PostVar::parse(None);
            let page = PageVar::parse(None);
            let loaded = query.load_id(
                flags(&attach, Some(b"hello"), post, page),
                post,
                page,
                &attach,
            );
            assert!(loaded.as_ref().is_ok_and(Vec::is_empty));
        }
    }
}

#[test]
fn a_miss_is_empty() {
    let store = Scripted::new([(
        get_sql(12),
        Reply::Rows(ResultSet::new([Column::new("ID")], Vec::<Row>::new())),
    )]);
    let prefix = TablePrefix::new("wp_");
    assert!(prefix.is_ok());
    if let Ok(prefix) = prefix {
        let opened = Query::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut query) = opened {
            let attach = AttachmentVar::parse(AttachmentInput::new());
            let post = PostVar::parse(Some(b"12"));
            let page = PageVar::parse(None);
            let loaded = query.load_id(flags(&attach, None, post, page), post, page, &attach);
            assert!(loaded.as_ref().is_ok_and(Vec::is_empty));
            let shown = format!("{query:?}");
            assert!(shown.contains("Query"));
        }
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let store = Scripted::new([]);
    let prefix = TablePrefix::new("wp_");
    assert!(prefix.is_ok());
    if let Ok(prefix) = prefix {
        let opened = Query::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut query) = opened {
            let attach = AttachmentVar::parse(AttachmentInput::new());
            let post = PostVar::parse(Some(b"12"));
            let page = PageVar::parse(None);
            let loaded = query.load_id(flags(&attach, None, post, page), post, page, &attach);
            assert!(loaded.is_err_and(|error| error.is_unavailable()));
        }
    }
}
