//! A store that returns programmed replies. For tests only (`test-util`).

use std::collections::VecDeque;

use crate::{Reply, Sql, Store, StoreError};

/// Replies to a fixed list of statements, in order.
#[derive(Debug)]
pub struct Scripted {
    remaining: VecDeque<(Sql, Reply)>,
    seen: Vec<Sql>,
}

impl Scripted {
    /// A store that answers `steps` in order.
    #[must_use]
    pub fn new(steps: impl IntoIterator<Item = (Sql, Reply)>) -> Self {
        Self {
            remaining: steps.into_iter().collect(),
            seen: Vec::new(),
        }
    }

    /// A store that answers one statement.
    #[must_use]
    pub fn once(sql: Sql, reply: Reply) -> Self {
        Self::new([(sql, reply)])
    }

    /// Statements that have already been executed.
    #[must_use]
    pub fn seen(&self) -> &[Sql] {
        &self.seen
    }
}

impl Store for Scripted {
    fn query(&mut self, sql: &Sql) -> Result<Reply, StoreError> {
        let Some((expected, reply)) = self.remaining.pop_front() else {
            return Err(StoreError::Unavailable);
        };
        if expected != *sql {
            self.remaining.push_front((expected, reply));
            return Err(StoreError::Unavailable);
        }
        self.seen.push(sql.clone());
        Ok(reply)
    }
}
