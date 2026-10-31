//! The SQL store port.

use crate::{Reply, Sql, StoreError};

/// Executes one SQL statement and returns the server's reply.
///
/// Implementations live in infrastructure crates. This trait is request-affine: a store holds one
/// connection's state and is not assumed to be [`Send`]. Contract: `contracts/db/store.md`.
pub trait Store {
    /// Runs `sql` unchanged.
    ///
    /// # Errors
    ///
    /// [`StoreError`] when the adapter cannot talk to a server. A server-reported SQL failure is
    /// [`Reply::Failed`].
    fn query(&mut self, sql: &Sql) -> Result<Reply, StoreError>;
}

/// Shared cases every [`Store`] impl must pass. `make` builds a store that replies to the given
/// script in order.
///
/// # Panics
///
/// Panics when the store does not return the scripted rows, write acknowledgement or server error.
#[cfg(feature = "test-util")]
pub fn store_contract<S: Store>(
    make: impl FnOnce(Vec<(Sql, Reply)>) -> S,
    extra: impl FnOnce(&mut S),
) {
    let select = Sql::from("SELECT option_value FROM fp_options WHERE option_name = 'home'");
    let insert = Sql::from("INSERT INTO fp_options (option_name, option_value) VALUES ('k','v')");
    let bad = Sql::from("SELECT * FROM missing");
    let mut store = make(vec![
        (
            select.clone(),
            Reply::single_column("option_value", [crate::Cell::bytes("https://example.test")]),
        ),
        (
            insert.clone(),
            Reply::Done {
                affected: 1,
                insert_id: 12,
            },
        ),
        (
            bad.clone(),
            Reply::Failed(crate::ServerError::new(
                1146,
                "Table 'reference.missing' doesn't exist",
            )),
        ),
    ]);

    let rows = store.query(&select);
    assert!(rows.as_ref().is_ok_and(|reply| reply.as_rows().is_some()));
    if let Ok(reply) = rows
        && let Some(set) = reply.as_rows()
    {
        assert_eq!(set.columns()[0].as_bytes(), b"option_value");
        assert_eq!(
            set.rows()[0].cells(),
            [crate::Cell::bytes("https://example.test")]
        );
    }

    let inserted = store.query(&insert);
    assert_eq!(
        inserted.as_ref().ok().and_then(Reply::as_done),
        Some((1, 12))
    );

    let failed = store.query(&bad);
    assert!(
        failed
            .as_ref()
            .is_ok_and(|reply| { reply.as_failed().is_some_and(|error| error.code() == 1146) })
    );
    extra(&mut store);
}
