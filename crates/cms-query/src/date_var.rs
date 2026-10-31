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

/// Archive-date flags after `parse_query` order (`contracts/query/date.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateFlags {
    archive: DateArchive,
    not_found: bool,
}

impl DateFlags {
    /// Resolve flags in day, then month, then year order.
    ///
    /// Time keys are treated as unset. A 404 from one step does not stop the
    /// next.
    #[must_use]
    pub fn resolve(parts: DateParts) -> Self {
        let mut archive = DateArchive::None;
        let mut not_found = false;
        if parts.day > 0 {
            if parts.monthnum > 0
                && parts.year > 0
                && !checkdate(parts.monthnum, parts.day, parts.year)
            {
                not_found = true;
            } else {
                archive = DateArchive::Day;
            }
        }
        if parts.monthnum > 0 && matches!(archive, DateArchive::None) {
            if parts.monthnum > 12 {
                not_found = true;
            } else {
                archive = DateArchive::Month;
            }
        }
        if parts.year > 0 && matches!(archive, DateArchive::None) {
            archive = DateArchive::Year;
        }
        Self { archive, not_found }
    }

    /// The winning archive kind.
    #[must_use]
    pub const fn archive(self) -> DateArchive {
        self.archive
    }

    /// Whether this is any date archive (`is_day`, `is_month`, or `is_year`).
    #[must_use]
    pub const fn is_date(self) -> bool {
        !matches!(self.archive, DateArchive::None)
    }

    /// Whether this is a year archive.
    #[must_use]
    pub const fn is_year(self) -> bool {
        matches!(self.archive, DateArchive::Year)
    }

    /// Whether this is a month archive.
    #[must_use]
    pub const fn is_month(self) -> bool {
        matches!(self.archive, DateArchive::Month)
    }

    /// Whether this is a day archive.
    #[must_use]
    pub const fn is_day(self) -> bool {
        matches!(self.archive, DateArchive::Day)
    }

    /// Whether resolve set the 404 error.
    #[must_use]
    pub const fn is_not_found(self) -> bool {
        self.not_found
    }
}

fn absint(bytes: &[u8]) -> i64 {
    php_int(bytes).saturating_abs()
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
    use super::{DateFlags, DateParts};

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
}
