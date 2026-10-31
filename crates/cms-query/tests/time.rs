//! Parse m, w, hour, minute and second (`contracts/query/time.md`).

use cms_query::{DateArchive, DateFlags, DateParts, StampVar, TimeParts, WeekVar};

#[derive(Clone, Copy, Default)]
struct Keys<'a> {
    year: Option<&'a [u8]>,
    monthnum: Option<&'a [u8]>,
    day: Option<&'a [u8]>,
    stamp: Option<&'a [u8]>,
    week: Option<&'a [u8]>,
    hour: Option<&'a [u8]>,
    minute: Option<&'a [u8]>,
    second: Option<&'a [u8]>,
}

fn flags(keys: Keys<'_>) -> DateFlags {
    DateFlags::resolve_all(
        &DateParts::parse(keys.year, keys.monthnum, keys.day),
        &StampVar::parse(keys.stamp),
        WeekVar::parse(keys.week),
        TimeParts::parse(keys.hour, keys.minute, keys.second),
    )
}

#[test]
fn missing_empty_are_not_time() {
    let none = flags(Keys::default());
    assert!(!none.is_time());
    assert!(!none.is_date());
    assert!(matches!(none.archive(), DateArchive::None));
    let empty = flags(Keys {
        stamp: Some(b""),
        week: Some(b""),
        hour: Some(b""),
        minute: Some(b""),
        second: Some(b""),
        ..Keys::default()
    });
    assert!(!empty.is_time());
    assert!(!empty.is_date());
    assert_eq!(TimeParts::parse(None, Some(b""), None).minute(), None);
}

#[test]
fn present_clock_is_time() {
    let hour = flags(Keys {
        hour: Some(b"0"),
        ..Keys::default()
    });
    assert!(hour.is_time());
    assert!(hour.is_date());
    assert!(!hour.is_year());
    assert!(!hour.is_not_found());
    assert_eq!(TimeParts::parse(Some(b"0"), None, None).hour(), Some(0));
    let minute = flags(Keys {
        minute: Some(b" 5"),
        ..Keys::default()
    });
    assert!(minute.is_time());
    let second = flags(Keys {
        second: Some(b"foo"),
        ..Keys::default()
    });
    assert!(second.is_time());
    assert_eq!(TimeParts::parse(None, None, Some(b"foo")).second(), Some(0));
}

#[test]
fn time_blocks_calendar_404() {
    let blocked = flags(Keys {
        year: Some(b"2026"),
        monthnum: Some(b"2"),
        day: Some(b"29"),
        hour: Some(b"12"),
        ..Keys::default()
    });
    assert!(blocked.is_time());
    assert!(blocked.is_date());
    assert!(!blocked.is_day());
    assert!(!blocked.is_month());
    assert!(!blocked.is_not_found());
    let open = flags(Keys {
        year: Some(b"2026"),
        monthnum: Some(b"2"),
        day: Some(b"29"),
        ..Keys::default()
    });
    assert!(open.is_month());
    assert!(open.is_not_found());
    assert!(!open.is_time());
}

#[test]
fn stamp_length_picks_the_flag() {
    let year = flags(Keys {
        stamp: Some(b"2026"),
        ..Keys::default()
    });
    assert!(year.is_year());
    assert!(year.is_date());
    assert!(!year.is_time());
    let month = flags(Keys {
        stamp: Some(b"2026-05"),
        ..Keys::default()
    });
    assert_eq!(StampVar::parse(Some(b"2026-05")).as_bytes(), b"202605");
    assert!(month.is_month());
    assert!(!month.is_year());
    let day = flags(Keys {
        stamp: Some(b"20260515"),
        ..Keys::default()
    });
    assert!(day.is_day());
    assert!(!day.is_month());
    let time = flags(Keys {
        stamp: Some(b"2026051512"),
        ..Keys::default()
    });
    assert!(time.is_time());
    assert!(!time.is_day());
}

#[test]
fn stamp_zero_is_not_a_date() {
    let zero = flags(Keys {
        stamp: Some(b"0"),
        ..Keys::default()
    });
    assert!(!zero.is_date());
    assert!(!StampVar::parse(Some(b"0")).is_set());
    let zeros = flags(Keys {
        stamp: Some(b"00"),
        ..Keys::default()
    });
    assert!(zeros.is_year());
    assert!(zeros.is_date());
    assert!(!StampVar::parse(Some(b"abc")).is_set());
}

#[test]
fn week_is_date_without_calendar() {
    let week = flags(Keys {
        week: Some(b"12"),
        ..Keys::default()
    });
    assert!(week.is_date());
    assert!(!week.is_year());
    assert!(!week.is_month());
    assert!(!week.is_day());
    assert!(!week.is_time());
    assert!(matches!(week.archive(), DateArchive::None));
    assert_eq!(WeekVar::parse(Some(b"-3")).value(), 3);
    let none = flags(Keys {
        week: Some(b"0"),
        ..Keys::default()
    });
    assert!(!none.is_date());
}

#[test]
fn stamp_stacks_on_time() {
    let stacked = flags(Keys {
        stamp: Some(b"202605"),
        hour: Some(b"12"),
        ..Keys::default()
    });
    assert!(stacked.is_time());
    assert!(stacked.is_month());
    assert!(stacked.is_date());
    assert!(!stacked.is_year());
}
