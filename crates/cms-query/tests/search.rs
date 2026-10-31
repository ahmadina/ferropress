//! Parse the `s` query var (`contracts/query/search.md`).

use cms_query::SearchVar;

#[test]
fn missing_is_not_search() {
    let missing = SearchVar::parse(None);
    assert_eq!(missing.as_bytes(), b"");
    assert!(!missing.is_search());
}

#[test]
fn empty_present_is_search() {
    let empty = SearchVar::parse(Some(b""));
    assert_eq!(empty.as_bytes(), b"");
    assert!(empty.is_search());
}

#[test]
fn bytes_are_kept_without_trim() {
    let parsed = SearchVar::parse(Some(b" hello "));
    assert_eq!(parsed.as_bytes(), b" hello ");
    assert!(parsed.is_search());
    let zero = SearchVar::parse(Some(b"0"));
    assert_eq!(zero.as_bytes(), b"0");
    assert!(zero.is_search());
}

#[test]
fn overlong_is_wiped_and_still_search() {
    let kept = vec![b'x'; 1600];
    let parsed = SearchVar::parse(Some(&kept));
    assert_eq!(parsed.as_bytes().len(), 1600);
    assert!(parsed.is_search());
    let wiped = vec![b'x'; 1601];
    let parsed = SearchVar::parse(Some(&wiped));
    assert_eq!(parsed.as_bytes(), b"");
    assert!(parsed.is_search());
}
