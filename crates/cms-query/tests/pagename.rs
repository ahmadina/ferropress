//! Parse the `pagename` query var (`contracts/query/pagename.md`).

use cms_query::PageNameVar;

#[test]
fn missing_empty_and_whitespace_are_not_a_page() {
    let missing = PageNameVar::parse(None);
    assert_eq!(missing.as_bytes(), b"");
    assert!(!missing.is_page());
    let empty = PageNameVar::parse(Some(b""));
    assert_eq!(empty.as_bytes(), b"");
    assert!(!empty.is_page());
    let spaces = PageNameVar::parse(Some(b" \n\t"));
    assert_eq!(spaces.as_bytes(), b"");
    assert!(!spaces.is_page());
}

#[test]
fn trimmed_slug_is_a_page() {
    let parsed = PageNameVar::parse(Some(b" about "));
    assert_eq!(parsed.as_bytes(), b"about");
    assert!(parsed.is_page());
}

#[test]
fn zero_bytes_are_a_page() {
    let parsed = PageNameVar::parse(Some(b"0"));
    assert_eq!(parsed.as_bytes(), b"0");
    assert!(parsed.is_page());
}

#[test]
fn inner_spaces_are_kept() {
    assert_eq!(PageNameVar::parse(Some(b"a b")).as_bytes(), b"a b");
    assert!(PageNameVar::parse(Some(b"a b")).is_page());
}
