//! Parse the `name` query var (`contracts/query/name.md`).

use cms_query::NameVar;

#[test]
fn missing_empty_and_whitespace_are_not_single() {
    let missing = NameVar::parse(None);
    assert_eq!(missing.as_bytes(), b"");
    assert!(!missing.is_single());
    let empty = NameVar::parse(Some(b""));
    assert_eq!(empty.as_bytes(), b"");
    assert!(!empty.is_single());
    let spaces = NameVar::parse(Some(b" \n\t"));
    assert_eq!(spaces.as_bytes(), b"");
    assert!(!spaces.is_single());
}

#[test]
fn trimmed_slug_is_single() {
    let parsed = NameVar::parse(Some(b" hello "));
    assert_eq!(parsed.as_bytes(), b"hello");
    assert!(parsed.is_single());
}

#[test]
fn zero_bytes_are_single() {
    let parsed = NameVar::parse(Some(b"0"));
    assert_eq!(parsed.as_bytes(), b"0");
    assert!(parsed.is_single());
}

#[test]
fn inner_spaces_are_kept() {
    assert_eq!(NameVar::parse(Some(b"a b")).as_bytes(), b"a b");
    assert!(NameVar::parse(Some(b"a b")).is_single());
}
