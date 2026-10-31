//! Load the post row for a name slug (`contracts/query/load-name.md`).

use cms_db::{Cell, Column, Reply, ResultSet, Row, Scripted, Sql, TablePrefix};
use cms_query::{
    AttachmentInput, AttachmentVar, NameVar, PageNameVar, PageVar, PostVar, Query, Singular,
};

fn name_sql(slug: &str) -> Sql {
    Sql::from(format!("SELECT * FROM wp_posts WHERE post_name = '{slug}' LIMIT 1").as_str())
}

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
    page_name: Option<&[u8]>,
    page: PageVar,
) -> Singular {
    Singular::resolve(
        attachment,
        &NameVar::parse(name),
        post,
        &PageNameVar::parse(page_name),
        page,
    )
}

#[test]
fn name_loads_the_row() {
    let store = Scripted::new([(name_sql("hello"), row("12", "Hello"))]);
    let prefix = TablePrefix::new("wp_");
    assert!(prefix.is_ok());
    if let Ok(prefix) = prefix {
        let opened = Query::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut query) = opened {
            let attach = AttachmentVar::parse(AttachmentInput::new());
            let post = PostVar::parse(None);
            let page = PageVar::parse(None);
            let name = NameVar::parse(Some(b"hello"));
            let page_name = PageNameVar::parse(None);
            let loaded = query.load(
                flags(&attach, Some(b"hello"), post, None, page),
                post,
                page,
                &attach,
                &name,
                &page_name,
            );
            assert!(loaded.as_ref().is_ok_and(|rows| {
                rows.len() == 1 && rows[0].id().as_u64() == 12 && rows[0].title() == b"Hello"
            }));
        }
    }
}

#[test]
fn attachment_slug_uses_basename() {
    let store = Scripted::new([(name_sql("file"), row("9", "File"))]);
    let prefix = TablePrefix::new("wp_");
    assert!(prefix.is_ok());
    if let Ok(prefix) = prefix {
        let opened = Query::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut query) = opened {
            let attach = AttachmentVar::parse(AttachmentInput::new().attachment(b"dir/file"));
            let post = PostVar::parse(None);
            let page = PageVar::parse(None);
            let name = NameVar::parse(None);
            let loaded = query.load_name(flags(&attach, None, post, None, page), &name, &attach);
            assert!(loaded.as_ref().is_ok_and(|rows| {
                rows.len() == 1 && rows[0].id().as_u64() == 9 && rows[0].title() == b"File"
            }));
        }
    }
}

#[test]
fn p_does_not_issue_name_sql() {
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
            let name = NameVar::parse(Some(b"hello"));
            let page_name = PageNameVar::parse(None);
            let loaded = query.load(
                flags(&attach, Some(b"hello"), post, None, page),
                post,
                page,
                &attach,
                &name,
                &page_name,
            );
            assert!(loaded.as_ref().is_ok_and(|rows| {
                rows.len() == 1 && rows[0].id().as_u64() == 12 && rows[0].title() == b"Hello"
            }));
        }
    }
}

#[test]
fn load_name_skips_page_flags() {
    let store = Scripted::new([]);
    let prefix = TablePrefix::new("wp_");
    assert!(prefix.is_ok());
    if let Ok(prefix) = prefix {
        let opened = Query::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut query) = opened {
            let attach = AttachmentVar::parse(AttachmentInput::new());
            let name = NameVar::parse(Some(b"hello"));
            let loaded = query.load_name(Singular::Page, &name, &attach);
            assert!(loaded.as_ref().is_ok_and(Vec::is_empty));
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
            let name = NameVar::parse(Some(b"hello"));
            let loaded = query.load_name(Singular::Single, &name, &attach);
            assert!(loaded.is_err_and(|error| error.is_unavailable()));
        }
    }
}
