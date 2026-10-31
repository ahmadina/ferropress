//! Map driver values onto the store port without rewriting SQL.

use cms_db::{Cell, Reply, ServerError, Sql};
use mysql::{Row, Value};

use crate::Error;

/// The driver takes `&str`; non-UTF-8 statement bytes are a platform failure.
pub(crate) fn sql_text(sql: &Sql) -> Result<&str, Error> {
    match std::str::from_utf8(sql.as_bytes()) {
        Ok(text) => Ok(text),
        Err(_) => Err(Error::Unavailable),
    }
}

/// A server packet becomes [`Reply::Failed`]; every other driver error is unreachable.
pub(crate) fn from_driver(error: mysql::Error) -> Result<Reply, Error> {
    match error {
        mysql::Error::MySqlError(server) => {
            Ok(Reply::Failed(ServerError::new(server.code, server.message)))
        }
        mysql::Error::IoError(_)
        | mysql::Error::CodecError(_)
        | mysql::Error::DriverError(_)
        | mysql::Error::UrlError(_)
        | mysql::Error::FromValueError(_)
        | mysql::Error::FromRowError(_) => Err(Error::Unavailable),
    }
}

/// Cells in column order. A missing cell means the driver dropped a field.
pub(crate) fn cells_from_row(row: &Row) -> Result<Vec<Cell>, Error> {
    let mut cells = Vec::with_capacity(row.len());
    for index in 0..row.len() {
        let Some(value) = row.as_ref(index) else {
            return Err(Error::Unavailable);
        };
        cells.push(cell_from_value(value));
    }
    Ok(cells)
}

/// One field: SQL `NULL` or the text the protocol would have sent.
pub(crate) fn cell_from_value(value: &Value) -> Cell {
    match value {
        Value::NULL => Cell::Null,
        Value::Bytes(bytes) => Cell::bytes(bytes),
        Value::Int(number) => Cell::bytes(number.to_string()),
        Value::UInt(number) => Cell::bytes(number.to_string()),
        Value::Float(number) => Cell::bytes(format!("{number}")),
        Value::Double(number) => Cell::bytes(format!("{number}")),
        Value::Date(year, month, day, hour, minute, second, micros) => Cell::bytes(datetime_text(
            *year, *month, *day, *hour, *minute, *second, *micros,
        )),
        Value::Time(negative, days, hours, minutes, seconds, micros) => Cell::bytes(time_text(
            *negative, *days, *hours, *minutes, *seconds, *micros,
        )),
    }
}

/// `YYYY-MM-DD HH:MM:SS` or with microseconds, matching the text protocol.
fn datetime_text(
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    micros: u32,
) -> String {
    if micros == 0 {
        format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
    } else {
        format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{micros:06}")
    }
}

/// `[-]HH:MM:SS` (hours include whole days), matching the text protocol.
fn time_text(
    negative: bool,
    days: u32,
    hours: u8,
    minutes: u8,
    seconds: u8,
    micros: u32,
) -> String {
    let hours = days.saturating_mul(24).saturating_add(u32::from(hours));
    let sign = if negative { "-" } else { "" };
    if micros == 0 {
        format!("{sign}{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{sign}{hours:02}:{minutes:02}:{seconds:02}.{micros:06}")
    }
}

#[cfg(test)]
mod tests {
    use super::{cell_from_value, from_driver, sql_text};
    use cms_db::{Cell, Sql};
    use mysql::{MySqlError, Value};

    #[test]
    fn a_server_packet_is_a_failed_reply() {
        let error = mysql::Error::MySqlError(MySqlError {
            state: "42S02".into(),
            message: "Table 'reference.missing' doesn't exist".into(),
            code: 1146,
        });
        let reply = from_driver(error);
        assert!(reply.as_ref().is_ok_and(|value| {
            value
                .as_failed()
                .is_some_and(|failed| failed.code() == 1146)
        }));
        if let Ok(reply) = reply
            && let Some(failed) = reply.as_failed()
        {
            assert_eq!(failed.message(), "Table 'reference.missing' doesn't exist");
        }
    }

    #[test]
    fn driver_failures_are_unavailable() {
        let io = mysql::Error::IoError(std::io::Error::other("offline"));
        assert!(from_driver(io).is_err());
        assert!(from_driver(mysql::Error::FromValueError(Value::NULL)).is_err());
    }

    #[test]
    fn non_utf8_sql_is_unavailable() {
        assert!(sql_text(&Sql::from("SELECT 1")).is_ok());
        assert!(sql_text(&Sql::new([0xff])).is_err());
    }

    #[test]
    fn cells_follow_the_text_the_server_would_send() {
        assert_eq!(cell_from_value(&Value::NULL), Cell::Null);
        assert_eq!(
            cell_from_value(&Value::Bytes(b"home".to_vec())),
            Cell::bytes("home")
        );
        assert_eq!(cell_from_value(&Value::Int(-3)), Cell::bytes("-3"));
        assert_eq!(cell_from_value(&Value::UInt(12)), Cell::bytes("12"));
        assert_eq!(
            cell_from_value(&Value::Date(2026, 1, 2, 3, 4, 5, 0)),
            Cell::bytes("2026-01-02 03:04:05")
        );
        assert_eq!(
            cell_from_value(&Value::Date(2026, 1, 2, 3, 4, 5, 7)),
            Cell::bytes("2026-01-02 03:04:05.000007")
        );
        assert_eq!(
            cell_from_value(&Value::Time(true, 1, 2, 3, 4, 0)),
            Cell::bytes("-26:03:04")
        );
        assert_eq!(
            cell_from_value(&Value::Time(false, 0, 1, 2, 3, 9)),
            Cell::bytes("01:02:03.000009")
        );
        assert_eq!(cell_from_value(&Value::Float(1.5)), Cell::bytes("1.5"));
        assert_eq!(cell_from_value(&Value::Double(2.5)), Cell::bytes("2.5"));
    }
}
