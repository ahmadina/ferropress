//! Parse the `page_id` query var (`contracts/query/page-id.md`).

use cms_query::PageVar;

#[test]
fn missing_empty_and_zero_are_not_a_page() {
    let missing = PageVar::parse(None);
    assert_eq!(missing.value(), 0);
    assert!(!missing.is_page());
    let empty = PageVar::parse(Some(b""));
    assert_eq!(empty.value(), 0);
    assert!(!empty.is_page());
    let zero = PageVar::parse(Some(b"0"));
    assert_eq!(zero.value(), 0);
    assert!(!zero.is_page());
}

#[test]
fn positive_is_a_page() {
    let parsed = PageVar::parse(Some(b"12"));
    assert_eq!(parsed.value(), 12);
    assert!(parsed.is_page());
}

#[test]
fn negative_becomes_absolute() {
    let parsed = PageVar::parse(Some(b"-4"));
    assert_eq!(parsed.value(), 4);
    assert!(parsed.is_page());
}

#[test]
fn leading_numeric_uses_absint() {
    assert_eq!(PageVar::parse(Some(b"12abc")).value(), 12);
    assert_eq!(PageVar::parse(Some(b" 9")).value(), 9);
    assert_eq!(PageVar::parse(Some(b"-3foo")).value(), 3);
    assert_eq!(PageVar::parse(Some(b"foo")).value(), 0);
    assert!(!PageVar::parse(Some(b"foo")).is_page());
}
