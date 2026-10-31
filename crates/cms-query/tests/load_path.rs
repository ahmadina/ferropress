//! Load the post row for a pagename path (`contracts/query/load-path.md`).

use cms_db::{Cell, Column, Reply, ResultSet, Row, Scripted, Sql, TablePrefix};
use cms_query::{
    AttachmentInput, AttachmentVar, NameVar, PageNameVar, PageVar, PostVar, Query, Singular,
};

fn path_sql(names: &str) -> Sql {
    Sql::from(
        format!(
            "\n\t\tSELECT ID, post_name, post_parent, post_type\n\t\tFROM wp_posts\n\t\tWHERE post_name IN ({names})\n\t\tAND post_type IN ('page','attachment')\n\t"
        )
        .as_str(),
    )
}

fn get_sql(id: u64) -> Sql {
    Sql::from(format!("SELECT * FROM wp_posts WHERE ID = {id} LIMIT 1").as_str())
}

fn candidates(id: &str, name: &str) -> Reply {
    Reply::Rows(ResultSet::new(
        [
            Column::new("ID"),
            Column::new("post_name"),
            Column::new("post_parent"),
            Column::new("post_type"),
        ],
        [Row::new([
            Cell::bytes(id),
            Cell::bytes(name),
            Cell::bytes("0"),
            Cell::bytes("page"),
        ])],
    ))
}

fn page_row(id: &str, title: &str) -> Reply {
    Reply::Rows(ResultSet::new(
        [Column::new("ID"), Column::new("post_title")],
        [Row::new([Cell::bytes(id), Cell::bytes(title)])],
    ))
}

fn flags(name: Option<&[u8]>, post: PostVar, page_name: Option<&[u8]>, page: PageVar) -> Singular {
    Singular::resolve(
        &AttachmentVar::parse(AttachmentInput::new()),
        &NameVar::parse(name),
        post,
        &PageNameVar::parse(page_name),
        page,
    )
}

#[test]
fn pagename_loads_the_row() {
    let store = Scripted::new([
        (path_sql("'about'"), candidates("4", "about")),
        (get_sql(4), page_row("4", "About")),
    ]);
    let prefix = TablePrefix::new("wp_");
    assert!(prefix.is_ok());
    if let Ok(prefix) = prefix {
        let opened = Query::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut query) = opened {
            let attach = AttachmentVar::parse(AttachmentInput::new());
            let post = PostVar::parse(None);
            let page = PageVar::parse(None);
            let page_name = PageNameVar::parse(Some(b"about"));
            let loaded = query.load(
                flags(None, post, Some(b"about"), page),
                post,
                page,
                &attach,
                &page_name,
            );
            assert!(loaded.as_ref().is_ok_and(|rows| {
                rows.len() == 1 && rows[0].id().as_u64() == 4 && rows[0].title() == b"About"
            }));
        }
    }
}

#[test]
fn page_id_does_not_issue_path_sql() {
    let store = Scripted::new([(get_sql(4), page_row("4", "About"))]);
    let prefix = TablePrefix::new("wp_");
    assert!(prefix.is_ok());
    if let Ok(prefix) = prefix {
        let opened = Query::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut query) = opened {
            let attach = AttachmentVar::parse(AttachmentInput::new());
            let post = PostVar::parse(None);
            let page = PageVar::parse(Some(b"4"));
            let page_name = PageNameVar::parse(Some(b"about"));
            let loaded = query.load(
                flags(None, post, Some(b"about"), page),
                post,
                page,
                &attach,
                &page_name,
            );
            assert!(loaded.as_ref().is_ok_and(|rows| {
                rows.len() == 1 && rows[0].id().as_u64() == 4 && rows[0].title() == b"About"
            }));
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
            let page_name = PageNameVar::parse(None);
            let loaded = query.load(
                flags(Some(b"hello"), post, None, page),
                post,
                page,
                &attach,
                &page_name,
            );
            assert!(loaded.as_ref().is_ok_and(Vec::is_empty));
        }
    }
}

#[test]
fn load_path_skips_non_page_flags() {
    let store = Scripted::new([]);
    let prefix = TablePrefix::new("wp_");
    assert!(prefix.is_ok());
    if let Ok(prefix) = prefix {
        let opened = Query::new(store, &prefix);
        assert!(opened.is_ok());
        if let Ok(mut query) = opened {
            let page_name = PageNameVar::parse(Some(b"about"));
            let loaded = query.load_path(Singular::None, &page_name);
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
            let page_name = PageNameVar::parse(Some(b"about"));
            let loaded = query.load_path(Singular::Page, &page_name);
            assert!(loaded.is_err_and(|error| error.is_unavailable()));
        }
    }
}
