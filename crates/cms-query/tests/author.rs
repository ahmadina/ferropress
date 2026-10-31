//! Parse `author` and `author_name` (`contracts/query/author.md`).

use cms_query::AuthorVar;

#[test]
fn missing_empty_and_zero_are_not_author() {
    let missing = AuthorVar::parse(None, None);
    assert_eq!(missing.as_bytes(), b"");
    assert!(!missing.is_author());
    assert!(!AuthorVar::parse(Some(b""), None).is_author());
    assert!(!AuthorVar::parse(Some(b"0"), None).is_author());
    assert!(!AuthorVar::parse(Some(b"abc"), None).is_author());
}

#[test]
fn numeric_zero_is_not_author() {
    assert!(!AuthorVar::parse(Some(b"00"), None).is_author());
    assert!(!AuthorVar::parse(Some(b"-0"), None).is_author());
    assert!(AuthorVar::parse(Some(b"0,1"), None).is_author());
    assert!(AuthorVar::parse(Some(b"-"), None).is_author());
    assert!(AuthorVar::parse(Some(b","), None).is_author());
    assert_eq!(AuthorVar::parse(Some(b"0,1"), None).as_bytes(), b"0,1");
}

#[test]
fn id_list_is_author() {
    let parsed = AuthorVar::parse(Some(b"1,2"), None);
    assert_eq!(parsed.as_bytes(), b"1,2");
    assert!(parsed.is_author());
    assert!(AuthorVar::parse(Some(b"-1"), None).is_author());
    assert!(AuthorVar::parse(Some(b"01"), None).is_author());
    assert_eq!(AuthorVar::parse(Some(b"user-3"), None).as_bytes(), b"-3");
    assert!(AuthorVar::parse(Some(b"user-3"), None).is_author());
}

#[test]
fn name_is_author_without_trim() {
    let named = AuthorVar::parse(Some(b"0"), Some(b" alice "));
    assert_eq!(named.name(), b" alice ");
    assert!(named.is_author());
    assert!(AuthorVar::parse(None, Some(b"0")).is_author());
    assert!(AuthorVar::parse(None, Some(b" ")).is_author());
    assert!(!AuthorVar::parse(None, Some(b"")).is_author());
}
