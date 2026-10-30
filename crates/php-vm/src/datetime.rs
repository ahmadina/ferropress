//! UTC `gmdate` / `date`, `time`, and a small `strtotime` for civil timestamps.

use php_compiler::bytecode::Operand;
use php_value::{PhpString, Value};

use crate::Output;
use crate::calls::Argument;
use crate::machine::{Exec, Machine};

impl<O: Output> Machine<'_, O> {
    pub(crate) fn native_gmdate(&mut self, args: &[Argument], result: Operand) -> Exec {
        self.format_stamp("gmdate", args, result)
    }

    pub(crate) fn native_date(&mut self, args: &[Argument], result: Operand) -> Exec {
        self.format_stamp("date", args, result)
    }

    fn format_stamp(&mut self, name: &'static str, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one(name, args)?;
        let format = self.string_arg(name, &value)?;
        let stamp = match args.get(1) {
            Some(arg) => self.compute(|cx| arg.slot.value().to_int(cx))?,
            None => current_unix(),
        };
        let text = format_utc(&format, stamp);
        self.write(result, Value::String(PhpString::new(&text)))
    }

    pub(crate) fn native_time(&mut self, args: &[Argument], result: Operand) -> Exec {
        if !args.is_empty() {
            return Err(self.too_many("time", args.len(), 0, args));
        }
        self.write(result, Value::Int(current_unix()))
    }

    pub(crate) fn native_strtotime(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("strtotime", args)?;
        let text = self.string_arg("strtotime", &value)?;
        let parsed = parse_utc(&text).map_or(Value::Bool(false), Value::Int);
        self.write(result, parsed)
    }
}

fn current_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
        .unwrap_or(0)
}

fn format_utc(format: &[u8], stamp: i64) -> Vec<u8> {
    let parts = utc_parts(stamp);
    let mut out = Vec::with_capacity(format.len() + 8);
    for &byte in format {
        match byte {
            b'Y' => out.extend_from_slice(format!("{:04}", parts.year).as_bytes()),
            b'm' => out.extend_from_slice(format!("{:02}", parts.month).as_bytes()),
            b'd' => out.extend_from_slice(format!("{:02}", parts.day).as_bytes()),
            b'H' => out.extend_from_slice(format!("{:02}", parts.hour).as_bytes()),
            b'i' => out.extend_from_slice(format!("{:02}", parts.minute).as_bytes()),
            b's' => out.extend_from_slice(format!("{:02}", parts.second).as_bytes()),
            other => out.push(other),
        }
    }
    out
}

fn parse_utc(bytes: &[u8]) -> Option<i64> {
    let text = core::str::from_utf8(bytes).ok()?.trim();
    let (date, time) = text.split_once(' ').unwrap_or((text, "00:00:00"));
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: u32 = date_parts.next()?.parse().ok()?;
    let day: u32 = date_parts.next()?.parse().ok()?;
    if date_parts.next().is_some() {
        return None;
    }
    let mut time_parts = time.split(':');
    let hour: u32 = time_parts.next()?.parse().ok()?;
    let minute: u32 = time_parts.next()?.parse().ok()?;
    let second: u32 = time_parts.next()?.parse().ok()?;
    if time_parts.next().is_some() {
        return None;
    }
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    Some(unix_from_utc(year, month, day, hour, minute, second))
}

#[derive(Debug, Clone, Copy)]
struct UtcParts {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

fn utc_parts(stamp: i64) -> UtcParts {
    let days = stamp.div_euclid(86_400);
    let tod = u32::try_from(stamp.rem_euclid(86_400)).unwrap_or(0);
    let (year, month, day) = civil_from_days(days);
    UtcParts {
        year,
        month,
        day,
        hour: tod / 3600,
        minute: (tod % 3600) / 60,
        second: tod % 60,
    }
}

fn unix_from_utc(year: i64, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
    days_from_civil(year, month, day) * 86_400
        + i64::from(hour) * 3600
        + i64::from(minute) * 60
        + i64::from(second)
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    (
        year,
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 }.div_euclid(400);
    let yoe = year - era * 400;
    let shifted = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * i64::from(shifted) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::{format_utc, parse_utc, unix_from_utc, utc_parts};

    #[test]
    fn epoch_is_the_first_of_nineteen_seventy() {
        let parts = utc_parts(0);
        assert_eq!((parts.year, parts.month, parts.day), (1970, 1, 1));
        assert_eq!(format_utc(b"Y-m-d H:i:s", 0), b"1970-01-01 00:00:00");
        assert_eq!(unix_from_utc(1970, 1, 1, 0, 0, 0), 0);
        assert_eq!(parse_utc(b"2021-01-01 00:00:00"), Some(1_609_459_200));
        assert_eq!(format_utc(b"Y-m-d", 1_609_459_200), b"2021-01-01");
        assert!(parse_utc(b"nope").is_none());
    }
}
