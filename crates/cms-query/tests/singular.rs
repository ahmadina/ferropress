//! Resolve singular flags (`contracts/query/singular.md`).

use cms_query::{AttachmentInput, AttachmentVar, NameVar, PageNameVar, PageVar, PostVar, Singular};

fn resolve(
    attachment: Option<&[u8]>,
    name: Option<&[u8]>,
    post: Option<&[u8]>,
    page_name: Option<&[u8]>,
    page: Option<&[u8]>,
) -> Singular {
    let attach = match attachment {
        Some(bytes) => AttachmentVar::parse(AttachmentInput::new().attachment(bytes)),
        None => AttachmentVar::parse(AttachmentInput::new()),
    };
    Singular::resolve(
        &attach,
        &NameVar::parse(name),
        PostVar::parse(post),
        &PageNameVar::parse(page_name),
        PageVar::parse(page),
    )
}

#[test]
fn empty_is_not_singular() {
    let parsed = resolve(None, None, None, None, None);
    assert!(!parsed.is_singular());
    assert!(!parsed.is_single());
    assert!(!parsed.is_page());
    assert!(!parsed.is_attachment());
}

#[test]
fn attachment_wins_over_later_keys() {
    let parsed = resolve(
        Some(b"file"),
        Some(b"n"),
        Some(b"1"),
        Some(b"p"),
        Some(b"2"),
    );
    assert!(parsed.is_attachment());
    assert!(parsed.is_single());
    assert!(!parsed.is_page());
    assert!(parsed.is_singular());
}

#[test]
fn name_and_p_win_over_page() {
    let name = resolve(None, Some(b"n"), Some(b"1"), Some(b"p"), Some(b"2"));
    assert!(name.is_single());
    assert!(!name.is_page());
    assert!(!name.is_attachment());
    assert!(name.is_singular());
    let post = resolve(None, None, Some(b"1"), Some(b"about"), Some(b"2"));
    assert!(post.is_single());
    assert!(!post.is_page());
}

#[test]
fn page_is_singular_and_not_single() {
    let parsed = resolve(None, None, Some(b"-3"), Some(b"about"), None);
    assert!(parsed.is_page());
    assert!(!parsed.is_single());
    assert!(!parsed.is_attachment());
    assert!(parsed.is_singular());
    let by_id = resolve(None, None, None, None, Some(b"8"));
    assert!(by_id.is_page());
    assert!(by_id.is_singular());
}
