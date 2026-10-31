//! One `MySQL` connection implementing [`cms_db::Store`].

use std::fmt;
use std::time::Duration;

use cms_db::{Column, Reply, ResultSet, Row, Sql, Store, StoreError};
use mysql::prelude::Queryable;
use mysql::{Conn, OptsBuilder};

use crate::convert::{cells_from_row, from_driver, sql_text};
use crate::{Endpoint, Error};

/// Give up quickly when nothing is listening (lab down, bad host).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// One TCP session. Request-affine: not assumed [`Send`].
pub struct Mysql {
    endpoint: Endpoint,
    conn: Conn,
}

impl Mysql {
    /// Open a TCP connection to `endpoint`.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the host refuses the connection or the handshake fails.
    pub fn connect(endpoint: &Endpoint) -> Result<Self, Error> {
        let opts = OptsBuilder::new()
            .ip_or_hostname(Some(endpoint.host()))
            .tcp_port(endpoint.port())
            .user(Some(endpoint.user()))
            .pass(Some(endpoint.password()))
            .db_name(Some(endpoint.database()))
            .prefer_socket(false)
            .tcp_connect_timeout(Some(CONNECT_TIMEOUT));
        let Ok(conn) = Conn::new(opts) else {
            return Err(Error::Unavailable);
        };
        Ok(Self {
            endpoint: endpoint.clone(),
            conn,
        })
    }

    /// Runs `sql` unchanged. Contract: `contracts/db/store.md`.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when the connection dies or `sql` is not UTF-8 (the driver
    /// takes `&str`). A syntax error or missing table is [`Reply::Failed`].
    pub fn query(&mut self, sql: &Sql) -> Result<Reply, Error> {
        let text = sql_text(sql)?;
        let result = match self.conn.query_iter(text) {
            Ok(result) => result,
            Err(error) => return from_driver(error),
        };
        let columns: Vec<Column> = result
            .columns()
            .as_ref()
            .iter()
            .map(|column| Column::new(column.name_ref()))
            .collect();
        if columns.is_empty() {
            return Ok(Reply::Done {
                affected: result.affected_rows(),
                insert_id: result.last_insert_id().unwrap_or_default(),
            });
        }
        let mut rows = Vec::new();
        for row in result {
            let row = match row {
                Ok(row) => row,
                Err(error) => return from_driver(error),
            };
            rows.push(Row::new(cells_from_row(&row)?));
        }
        Ok(Reply::Rows(ResultSet::new(columns, rows)))
    }
}

impl Store for Mysql {
    fn query(&mut self, sql: &Sql) -> Result<Reply, StoreError> {
        Mysql::query(self, sql).map_err(StoreError::from)
    }
}

impl fmt::Debug for Mysql {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mysql")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::Mysql;
    use crate::Endpoint;

    #[test]
    fn a_refused_port_is_unavailable() {
        let endpoint = Endpoint::new("127.0.0.1", 1, "reference", "reference", "reference");
        let opened = Mysql::connect(&endpoint);
        assert!(opened.is_err());
        if let Err(error) = opened {
            assert!(error.is_unavailable());
        }
    }

    #[test]
    fn debug_redacts_the_endpoint_password() {
        let endpoint = Endpoint::new("127.0.0.1", 1, "u", "hidden", "d");
        let shown = format!("{endpoint:?}");
        assert!(!shown.contains("hidden"));
        assert!(shown.contains("<redacted>"));
    }
}
