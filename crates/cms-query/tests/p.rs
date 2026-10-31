//! Parse the `p` query var (`contracts/query/p.md`).

use cms_query::PostVar;

#[test]
fn missing_empty_and_zero_are_not_single() {
    let missing = PostVar::parse(None);
    assert_eq!(missing.value(), 0);
    assert!(!missing.is_single());
    assert!(!missing.is_not_found());
    let empty = PostVar::parse(Some(b""));
    assert_eq!(empty.value(), 0);
    assert!(!empty.is_single());
    assert!(!empty.is_not_found());
    let zero = PostVar::parse(Some(b"0"));
    assert_eq!(zero.value(), 0);
    assert!(!zero.is_single());
    assert!(!zero.is_not_found());
}

#[test]
fn positive_is_single() {
    let parsed = PostVar::parse(Some(b"12"));
    assert_eq!(parsed.value(), 12);
    assert!(parsed.is_single());
    assert!(!parsed.is_not_found());
}

#[test]
fn negative_is_not_found() {
    let parsed = PostVar::parse(Some(b"-3"));
    assert_eq!(parsed.value(), 0);
    assert!(!parsed.is_single());
    assert!(parsed.is_not_found());
}

#[test]
fn leading_numeric_uses_php_int() {
    assert_eq!(PostVar::parse(Some(b"12abc")).value(), 12);
    assert_eq!(PostVar::parse(Some(b" 9")).value(), 9);
    assert_eq!(PostVar::parse(Some(b"foo")).value(), 0);
    assert!(!PostVar::parse(Some(b"foo")).is_not_found());
}
