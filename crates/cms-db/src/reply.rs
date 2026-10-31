//! What a store returns after one statement.

/// A server-reported SQL failure. This is a plugin-observable outcome, not a Rust [`Err`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerError {
    code: u16,
    message: Box<str>,
}

impl ServerError {
    /// An error with this server code and message.
    #[must_use]
    pub fn new(code: u16, message: impl Into<Box<str>>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// The numeric code the server sent.
    #[must_use]
    pub const fn code(&self) -> u16 {
        self.code
    }

    /// The message the server sent.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// A result-set column name, in the order the server sent columns.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Column(Box<[u8]>);

impl Column {
    /// A column named with these bytes.
    #[must_use]
    pub fn new(name: impl AsRef<[u8]>) -> Self {
        Self(name.as_ref().into())
    }

    /// The name as stored.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// One field of a row: SQL `NULL` or the text the server sent.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Cell {
    /// SQL `NULL`.
    Null,
    /// The field's bytes.
    Bytes(Box<[u8]>),
}

impl Cell {
    /// A non-null field with these bytes.
    #[must_use]
    pub fn bytes(value: impl AsRef<[u8]>) -> Self {
        Self::Bytes(value.as_ref().into())
    }
}

/// One result row. Cells line up with [`ResultSet::columns`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row(Vec<Cell>);

impl Row {
    /// A row with these cells.
    #[must_use]
    pub fn new(cells: impl Into<Vec<Cell>>) -> Self {
        Self(cells.into())
    }

    /// The cells in column order.
    #[must_use]
    pub fn cells(&self) -> &[Cell] {
        &self.0
    }
}

/// A `SELECT` (or other) result set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultSet {
    columns: Vec<Column>,
    rows: Vec<Row>,
}

impl ResultSet {
    /// A result set with these columns and rows.
    #[must_use]
    pub fn new(columns: impl Into<Vec<Column>>, rows: impl Into<Vec<Row>>) -> Self {
        Self {
            columns: columns.into(),
            rows: rows.into(),
        }
    }

    /// Column names in server order.
    #[must_use]
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// Rows in server order.
    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }
}

/// What the store produced for one statement.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reply {
    /// A result set.
    Rows(ResultSet),
    /// A write (or other non-row) acknowledgement.
    Done {
        /// Rows the statement affected.
        affected: u64,
        /// Last insert id, or `0` when the statement created none.
        insert_id: u64,
    },
    /// The server rejected the statement.
    Failed(ServerError),
}

impl Reply {
    /// A one-column result set from `(name, value)` pairs, one pair per row.
    #[must_use]
    pub fn single_column(name: impl AsRef<[u8]>, values: impl IntoIterator<Item = Cell>) -> Self {
        let rows: Vec<Row> = values
            .into_iter()
            .map(|cell| Row::new(vec![cell]))
            .collect();
        Self::Rows(ResultSet::new(vec![Column::new(name)], rows))
    }

    /// The result set when this is [`Self::Rows`].
    #[must_use]
    pub const fn as_rows(&self) -> Option<&ResultSet> {
        match self {
            Self::Rows(set) => Some(set),
            Self::Done { .. } | Self::Failed(_) => None,
        }
    }

    /// Affected-row count and insert id when this is [`Self::Done`].
    #[must_use]
    pub const fn as_done(&self) -> Option<(u64, u64)> {
        match self {
            Self::Done {
                affected,
                insert_id,
            } => Some((*affected, *insert_id)),
            Self::Rows(_) | Self::Failed(_) => None,
        }
    }

    /// The server error when this is [`Self::Failed`].
    #[must_use]
    pub const fn as_failed(&self) -> Option<&ServerError> {
        match self {
            Self::Failed(error) => Some(error),
            Self::Rows(_) | Self::Done { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Cell, Reply, ServerError};

    #[test]
    fn replies_expose_their_shape() {
        let rows = Reply::single_column("n", [Cell::bytes("1")]);
        assert!(rows.as_rows().is_some());
        if let Some(set) = rows.as_rows() {
            assert_eq!(set.columns()[0].as_bytes(), b"n");
            assert_eq!(set.rows()[0].cells(), [Cell::bytes("1")]);
        }
        assert!(rows.as_done().is_none());

        let done = Reply::Done {
            affected: 2,
            insert_id: 9,
        };
        assert_eq!(done.as_done(), Some((2, 9)));
        assert!(done.as_rows().is_none());

        let failed = Reply::Failed(ServerError::new(1146, "table missing"));
        assert!(failed.as_failed().is_some());
        if let Some(error) = failed.as_failed() {
            assert_eq!(error.code(), 1146);
            assert_eq!(error.message(), "table missing");
        }
    }
}
