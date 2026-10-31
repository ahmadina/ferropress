//! Year, monthnum and day after `parse_query`.

use php_value::PhpString;
use php_value::Value;
use php_value::ops::Collect;

/// The parsed `year`, `monthnum` and `day` query vars (`contracts/query/date.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateParts {
    year: i64,
    monthnum: i64,
    day: i64,
}

impl DateParts {
    /// Parse optional raw bytes the way `parse_query` reads the three keys.
    ///
    /// Missing or empty bytes are `0`. A scalar is `absint`.
    #[must_use]
    pub fn parse(year: Option<&[u8]>, monthnum: Option<&[u8]>, day: Option<&[u8]>) -> Self {
        Self {
            year: year.map_or(0, absint),
            monthnum: monthnum.map_or(0, absint),
            day: day.map_or(0, absint),
        }
    }

    /// The stored `year` after `absint`.
    #[must_use]
    pub const fn year(self) -> i64 {
        self.year
    }

    /// The stored `monthnum` after `absint`.
    #[must_use]
    pub const fn monthnum(self) -> i64 {
        self.monthnum
    }

    /// The stored `day` after `absint`.
    #[must_use]
    pub const fn day(self) -> i64 {
        self.day
    }
}

/// Which archive-date request won (`contracts/query/date.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DateArchive {
    /// No year, month, or day claimed the query.
    None,
    /// A year with no earlier day or month.
    Year,
    /// A month with no earlier day.
    Month,
    /// A day (calendar-checked when month and year are set).
    Day,
}

/// The parsed `m` query var (`contracts/query/time.md`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StampVar {
    digits: Box<[u8]>,
}

impl StampVar {
    /// Keep only ASCII digits from optional raw `m` bytes.
    ///
    /// Missing bytes store empty. The stored value is the digit string.
    #[must_use]
    pub fn parse(raw: Option<&[u8]>) -> Self {
        Self {
            digits: raw.map(digits_only).unwrap_or_default(),
        }
    }

    /// The stored digit bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.digits
    }

    /// Whether the stored digits are PHP-string-truthy (not empty and not `0`).
    #[must_use]
    pub fn is_set(&self) -> bool {
        php_string_truthy(&self.digits)
    }
}

impl Default for StampVar {
    fn default() -> Self {
        Self::parse(None)
    }
}

/// The parsed `w` query var (`contracts/query/time.md`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WeekVar {
    value: i64,
}

impl WeekVar {
    /// Parse optional raw bytes the way `parse_query` reads `w`.
    ///
    /// Missing or empty bytes are `0`. A scalar is `absint`.
    #[must_use]
    pub fn parse(raw: Option<&[u8]>) -> Self {
        Self {
            value: raw.map_or(0, absint),
        }
    }

    /// The stored `w` after `absint`.
    #[must_use]
    pub const fn value(self) -> i64 {
        self.value
    }
}

/// The parsed `hour`, `minute` and `second` query vars (`contracts/query/time.md`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TimeParts {
    hour: Option<i64>,
    minute: Option<i64>,
    second: Option<i64>,
}

impl TimeParts {
    /// Parse optional raw bytes the way `parse_query` reads the clock keys.
    ///
    /// Missing or empty bytes stay empty. Any other scalar is `absint`.
    #[must_use]
    pub fn parse(hour: Option<&[u8]>, minute: Option<&[u8]>, second: Option<&[u8]>) -> Self {
        Self {
            hour: clock(hour),
            minute: clock(minute),
            second: clock(second),
        }
    }

    /// The stored `hour`, or empty.
    #[must_use]
    pub const fn hour(self) -> Option<i64> {
        self.hour
    }

    /// The stored `minute`, or empty.
    #[must_use]
    pub const fn minute(self) -> Option<i64> {
        self.minute
    }

    /// The stored `second`, or empty.
    #[must_use]
    pub const fn second(self) -> Option<i64> {
        self.second
    }
}

const YEAR: u8 = 1;
const MONTH: u8 = 1 << 1;
const DAY: u8 = 1 << 2;
const TIME: u8 = 1 << 3;
const DATE: u8 = 1 << 4;
const NOT_FOUND: u8 = 1 << 5;

/// Archive-date flags after `parse_query` order (`contracts/query/date.md`,
/// `contracts/query/time.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateFlags {
    bits: u8,
}

impl DateFlags {
    /// Resolve calendar flags in day, then month, then year order.
    ///
    /// Stamp, week and time keys are treated as unset. A 404 from one step
    /// does not stop the next.
    #[must_use]
    pub fn resolve(parts: DateParts) -> Self {
        Self::resolve_all(
            &parts,
            &StampVar::parse(None),
            WeekVar::parse(None),
            TimeParts::parse(None, None, None),
        )
    }

    /// Resolve flags in clock, calendar, stamp, then week order.
    ///
    /// A present clock value sets `is_date` first, so the calendar 404 does
    /// not run. Stamp length adds a flag without clearing earlier ones.
    #[must_use]
    pub fn resolve_all(
        parts: &DateParts,
        stamp: &StampVar,
        week: WeekVar,
        time: TimeParts,
    ) -> Self {
        let mut bits = 0_u8;
        if time.second.is_some() {
            bits |= TIME | DATE;
        }
        if time.minute.is_some() {
            bits |= TIME | DATE;
        }
        if time.hour.is_some() {
            bits |= TIME | DATE;
        }
        if parts.day > 0 && (bits & DATE) == 0 {
            if parts.monthnum > 0
                && parts.year > 0
                && !checkdate(parts.monthnum, parts.day, parts.year)
            {
                bits |= NOT_FOUND;
            } else {
                bits |= DAY | DATE;
            }
        }
        if parts.monthnum > 0 && (bits & DATE) == 0 {
            if parts.monthnum > 12 {
                bits |= NOT_FOUND;
            } else {
                bits |= MONTH | DATE;
            }
        }
        if parts.year > 0 && (bits & DATE) == 0 {
            bits |= YEAR | DATE;
        }
        if stamp.is_set() {
            bits |= DATE;
            let len = stamp.digits.len();
            if len > 9 {
                bits |= TIME;
            } else if len > 7 {
                bits |= DAY;
            } else if len > 5 {
                bits |= MONTH;
            } else {
                bits |= YEAR;
            }
        }
        if week.value > 0 {
            bits |= DATE;
        }
        Self { bits }
    }

    /// The winning year, month or day archive.
    ///
    /// Time-only and week-only requests are [`DateArchive::None`] even when
    /// [`Self::is_date`] is true.
    #[must_use]
    pub const fn archive(self) -> DateArchive {
        if self.has(DAY) {
            DateArchive::Day
        } else if self.has(MONTH) {
            DateArchive::Month
        } else if self.has(YEAR) {
            DateArchive::Year
        } else {
            DateArchive::None
        }
    }

    /// Whether this is any date archive, including time-only and week-only.
    #[must_use]
    pub const fn is_date(self) -> bool {
        self.has(DATE)
    }

    /// Whether this is a year archive.
    #[must_use]
    pub const fn is_year(self) -> bool {
        self.has(YEAR)
    }

    /// Whether this is a month archive.
    #[must_use]
    pub const fn is_month(self) -> bool {
        self.has(MONTH)
    }

    /// Whether this is a day archive.
    #[must_use]
    pub const fn is_day(self) -> bool {
        self.has(DAY)
    }

    /// Whether this is a time archive.
    #[must_use]
    pub const fn is_time(self) -> bool {
        self.has(TIME)
    }

    /// Whether resolve set the 404 error.
    #[must_use]
    pub const fn is_not_found(self) -> bool {
        self.has(NOT_FOUND)
    }

    const fn has(self, flag: u8) -> bool {
        self.bits & flag != 0
    }
}

fn absint(bytes: &[u8]) -> i64 {
    php_int(bytes).saturating_abs()
}

fn clock(raw: Option<&[u8]>) -> Option<i64> {
    raw.filter(|bytes| !bytes.is_empty()).map(absint)
}

fn digits_only(bytes: &[u8]) -> Box<[u8]> {
    bytes.iter().copied().filter(u8::is_ascii_digit).collect()
}

fn php_string_truthy(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes != b"0"
}

fn php_int(bytes: &[u8]) -> i64 {
    let value = Value::from(PhpString::from(bytes));
    let mut cx = Collect::default();
    value.to_int(&mut cx).unwrap_or_default()
}

fn checkdate(month: i64, day: i64, year: i64) -> bool {
    let Ok(month) = u8::try_from(month) else {
        return false;
    };
    let Ok(day) = u8::try_from(day) else {
        return false;
    };
    let Ok(year) = u16::try_from(year) else {
        return false;
    };
    if !(1..=12).contains(&month) || !(1..=32767).contains(&year) || day == 0 {
        return false;
    }
    day <= days_in_month(month, year)
}

fn days_in_month(month: u8, year: u16) -> u8 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn is_leap(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

#[cfg(test)]
mod tests {
    use super::{DateFlags, DateParts, StampVar, TimeParts, WeekVar};

    #[test]
    fn parts_use_absint() {
        let missing = DateParts::parse(None, None, None);
        assert_eq!(missing.year(), 0);
        assert_eq!(missing.monthnum(), 0);
        assert_eq!(missing.day(), 0);
        let parsed = DateParts::parse(Some(b"-2026"), Some(b"13abc"), Some(b" 7"));
        assert_eq!(parsed.year(), 2026);
        assert_eq!(parsed.monthnum(), 13);
        assert_eq!(parsed.day(), 7);
    }

    #[test]
    fn flags_follow_day_month_year() {
        let none = DateFlags::resolve(DateParts::parse(None, None, None));
        assert!(!none.is_date());
        assert!(!none.is_not_found());
        let year = DateFlags::resolve(DateParts::parse(Some(b"2026"), None, None));
        assert!(year.is_year());
        assert!(year.is_date());
        assert!(!year.is_month());
        let month = DateFlags::resolve(DateParts::parse(Some(b"2026"), Some(b"5"), None));
        assert!(month.is_month());
        assert!(!month.is_year());
        let day = DateFlags::resolve(DateParts::parse(Some(b"2026"), Some(b"1"), Some(b"15")));
        assert!(day.is_day());
        assert!(!day.is_month());
        let leap = DateFlags::resolve(DateParts::parse(Some(b"2024"), Some(b"2"), Some(b"29")));
        assert!(leap.is_day());
        assert!(!leap.is_not_found());
        let bad_day = DateFlags::resolve(DateParts::parse(Some(b"2026"), Some(b"2"), Some(b"29")));
        assert!(bad_day.is_month());
        assert!(bad_day.is_not_found());
        let april = DateFlags::resolve(DateParts::parse(Some(b"2026"), Some(b"4"), Some(b"31")));
        assert!(april.is_month());
        assert!(april.is_not_found());
        let far = DateFlags::resolve(DateParts::parse(Some(b"32768"), Some(b"1"), Some(b"1")));
        assert!(far.is_month());
        assert!(far.is_not_found());
        let huge_year =
            DateFlags::resolve(DateParts::parse(Some(b"70000"), Some(b"1"), Some(b"1")));
        assert!(huge_year.is_month());
        assert!(huge_year.is_not_found());
        let huge_month =
            DateFlags::resolve(DateParts::parse(Some(b"2026"), Some(b"256"), Some(b"1")));
        assert!(huge_month.is_year());
        assert!(huge_month.is_not_found());
        let huge_day =
            DateFlags::resolve(DateParts::parse(Some(b"2026"), Some(b"1"), Some(b"256")));
        assert!(huge_day.is_month());
        assert!(huge_day.is_not_found());
        let bad_month = DateFlags::resolve(DateParts::parse(Some(b"2026"), Some(b"13"), None));
        assert!(bad_month.is_year());
        assert!(bad_month.is_not_found());
        let day_only = DateFlags::resolve(DateParts::parse(None, None, Some(b"32")));
        assert!(day_only.is_day());
        assert!(!day_only.is_not_found());
    }

    #[test]
    fn stamp_keeps_digits_and_php_truth() {
        assert_eq!(StampVar::parse(None).as_bytes(), b"");
        assert!(!StampVar::parse(None).is_set());
        assert_eq!(StampVar::parse(Some(b"2026-05")).as_bytes(), b"202605");
        assert!(StampVar::parse(Some(b"2026-05")).is_set());
        assert!(!StampVar::parse(Some(b"0")).is_set());
        assert!(StampVar::parse(Some(b"00")).is_set());
        assert!(!StampVar::parse(Some(b"abc")).is_set());
    }

    #[test]
    fn week_and_clock_parse() {
        assert_eq!(WeekVar::parse(None).value(), 0);
        assert_eq!(WeekVar::parse(Some(b"-3")).value(), 3);
        let empty = TimeParts::parse(None, Some(b""), None);
        assert_eq!(empty.hour(), None);
        assert_eq!(empty.minute(), None);
        let zero = TimeParts::parse(Some(b"0"), Some(b" 5"), Some(b"foo"));
        assert_eq!(zero.hour(), Some(0));
        assert_eq!(zero.minute(), Some(5));
        assert_eq!(zero.second(), Some(0));
    }

    #[test]
    fn clock_stamp_and_week_flags() {
        let hour = DateFlags::resolve_all(
            &DateParts::parse(Some(b"2026"), Some(b"13"), Some(b"29")),
            &StampVar::parse(None),
            WeekVar::parse(None),
            TimeParts::parse(Some(b"0"), None, None),
        );
        assert!(hour.is_time());
        assert!(hour.is_date());
        assert!(!hour.is_year());
        assert!(!hour.is_not_found());
        let stamp = DateFlags::resolve_all(
            &DateParts::parse(None, None, None),
            &StampVar::parse(Some(b"20260515")),
            WeekVar::parse(None),
            TimeParts::parse(None, None, None),
        );
        assert!(stamp.is_day());
        assert!(!stamp.is_time());
        let week = DateFlags::resolve_all(
            &DateParts::parse(None, None, None),
            &StampVar::parse(None),
            WeekVar::parse(Some(b"12")),
            TimeParts::parse(None, None, None),
        );
        assert!(week.is_date());
        assert!(!week.is_year());
        assert!(!week.is_time());
    }
}
