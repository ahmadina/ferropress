//! Page path lookup (`contracts/content/path.md`). SQL text is the
//! `get_page_by_path` statement from the pinned reference.

use cms_content::Posts;
use cms_db::{Cell, Column, Reply, ResultSet, Row, Scripted, Sql, TablePrefix};

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

fn candidates(rows: &[(&str, &str, &str, &str)]) -> Reply {
    let body: Vec<Row> = rows
        .iter()
        .map(|(id, name, parent, type_name)| {
            Row::new([
                Cell::bytes(*id),
                Cell::bytes(*name),
                Cell::bytes(*parent),
                Cell::bytes(*type_name),
            ])
        })
        .collect();
    Reply::Rows(ResultSet::new(
        [
            Column::new("ID"),
            Column::new("post_name"),
            Column::new("post_parent"),
            Column::new("post_type"),
        ],
        body,
    ))
}

fn page_row(id: &str, title: &str) -> Reply {
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
fn about_loads_the_page_row() {
    let store = Scripted::new([
        (
            path_sql("'about'"),
            candidates(&[("4", "about", "0", "page")]),
        ),
        (get_sql(4), page_row("4", "About")),
    ]);
    let opened = open(store);
    assert!(opened.is_some());
    if let Some(mut posts) = opened {
        let loaded = posts.by_path(b"about");
        assert!(loaded.as_ref().is_ok_and(|post| {
            post.as_ref()
                .is_some_and(|post| post.id().as_u64() == 4 && post.title() == b"About")
        }));
    }
}

#[test]
fn nested_path_walks_parents() {
    let store = Scripted::new([
        (
            path_sql("'parent','child'"),
            candidates(&[("1", "parent", "0", "page"), ("2", "child", "1", "page")]),
        ),
        (get_sql(2), page_row("2", "Child")),
    ]);
    let opened = open(store);
    assert!(opened.is_some());
    if let Some(mut posts) = opened {
        let loaded = posts.by_path(b"parent/child");
        assert!(
            loaded
                .as_ref()
                .is_ok_and(|post| { post.as_ref().is_some_and(|post| post.id().as_u64() == 2) })
        );
    }
}

#[test]
fn page_type_wins_over_attachment() {
    let store = Scripted::new([
        (
            path_sql("'about'"),
            candidates(&[
                ("3", "about", "0", "attachment"),
                ("4", "about", "0", "page"),
            ]),
        ),
        (get_sql(4), page_row("4", "About")),
    ]);
    let opened = open(store);
    assert!(opened.is_some());
    if let Some(mut posts) = opened {
        let loaded = posts.by_path(b"About");
        assert!(
            loaded
                .as_ref()
                .is_ok_and(|post| { post.as_ref().is_some_and(|post| post.id().as_u64() == 4) })
        );
    }
}

#[test]
fn a_miss_is_empty() {
    let store = Scripted::new([(
        path_sql("'missing'"),
        Reply::Rows(ResultSet::new(
            [
                Column::new("ID"),
                Column::new("post_name"),
                Column::new("post_parent"),
                Column::new("post_type"),
            ],
            Vec::<Row>::new(),
        )),
    )]);
    let opened = open(store);
    assert!(opened.is_some());
    if let Some(mut posts) = opened {
        let loaded = posts.by_path(b"missing");
        assert!(loaded.as_ref().is_ok_and(Option::is_none));
    }
}

#[test]
fn a_failed_path_query_is_a_miss() {
    let store = Scripted::new([(
        path_sql("'about'"),
        Reply::Failed(cms_db::ServerError::new(1146, "missing")),
    )]);
    let opened = open(store);
    assert!(opened.is_some());
    if let Some(mut posts) = opened {
        let loaded = posts.by_path(b"about");
        assert!(loaded.as_ref().is_ok_and(Option::is_none));
    }
}

#[test]
fn unavailable_store_is_an_error() {
    let store = Scripted::new([]);
    let opened = open(store);
    assert!(opened.is_some());
    if let Some(mut posts) = opened {
        let loaded = posts.by_path(b"about");
        assert!(loaded.is_err_and(|error| error.is_unavailable()));
    }
}
