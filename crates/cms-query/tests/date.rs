//! Parse year, monthnum and day (`contracts/query/date.md`).

use cms_query::{DateArchive, DateFlags, DateParts};

#[test]
fn missing_empty_and_zero_are_not_a_date() {
    let parts = DateParts::parse(None, None, None);
    assert_eq!(parts.year(), 0);
    assert_eq!(parts.monthnum(), 0);
    assert_eq!(parts.day(), 0);
    let flags = DateFlags::resolve(parts);
    assert!(!flags.is_date());
    assert!(!flags.is_year());
    assert!(!flags.is_month());
    assert!(!flags.is_day());
    assert!(!flags.is_not_found());
    assert!(matches!(flags.archive(), DateArchive::None));
    let zeros = DateFlags::resolve(DateParts::parse(Some(b"0"), Some(b""), Some(b"0")));
    assert!(!zeros.is_date());
}

#[test]
fn year_only_is_a_year() {
    let flags = DateFlags::resolve(DateParts::parse(Some(b"2026"), None, None));
    assert!(flags.is_year());
    assert!(flags.is_date());
    assert!(!flags.is_month());
    assert!(!flags.is_day());
    assert!(!flags.is_not_found());
    assert!(matches!(flags.archive(), DateArchive::Year));
}

#[test]
fn month_wins_over_year() {
    let flags = DateFlags::resolve(DateParts::parse(Some(b"2026"), Some(b"5"), None));
    assert!(flags.is_month());
    assert!(flags.is_date());
    assert!(!flags.is_year());
    assert!(!flags.is_day());
    assert!(matches!(flags.archive(), DateArchive::Month));
    let abs = DateParts::parse(Some(b"-2026"), Some(b"-3"), None);
    assert_eq!(abs.year(), 2026);
    assert_eq!(abs.monthnum(), 3);
}

#[test]
fn valid_day_is_a_day() {
    let flags = DateFlags::resolve(DateParts::parse(Some(b"2024"), Some(b"2"), Some(b"29")));
    assert!(flags.is_day());
    assert!(flags.is_date());
    assert!(!flags.is_month());
    assert!(!flags.is_year());
    assert!(!flags.is_not_found());
    assert!(matches!(flags.archive(), DateArchive::Day));
    let day_only = DateFlags::resolve(DateParts::parse(None, None, Some(b"32")));
    assert!(day_only.is_day());
    assert!(!day_only.is_not_found());
}

#[test]
fn invalid_day_falls_through_to_month() {
    let flags = DateFlags::resolve(DateParts::parse(Some(b"2026"), Some(b"2"), Some(b"29")));
    assert!(flags.is_month());
    assert!(flags.is_date());
    assert!(!flags.is_day());
    assert!(flags.is_not_found());
    let century = DateFlags::resolve(DateParts::parse(Some(b"1900"), Some(b"2"), Some(b"29")));
    assert!(century.is_month());
    assert!(century.is_not_found());
}

#[test]
fn month_over_twelve_is_not_found() {
    let flags = DateFlags::resolve(DateParts::parse(Some(b"2026"), Some(b"13"), None));
    assert!(flags.is_year());
    assert!(flags.is_date());
    assert!(!flags.is_month());
    assert!(flags.is_not_found());
    let month_only = DateFlags::resolve(DateParts::parse(None, Some(b"13"), None));
    assert!(!month_only.is_date());
    assert!(month_only.is_not_found());
    assert_eq!(
        DateParts::parse(Some(b"12abc"), Some(b" 9"), Some(b"-3foo")).day(),
        3
    );
}
