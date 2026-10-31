//! Live `MySQL` replies (`contracts/db/store.md`). Uses a `TEMPORARY TABLE` so nothing persists.
//!
//! Expected codes come from `MySQL` 8.4 (lab image). When nothing is listening, the tests
//! return without asserting — unit tests cover mapping. Set `FERROPRESS_MYSQL_URL` to
//! `mysql://user:pass@host:port/db` to point at another server.

use cms_db::{Cell, Sql, Store};
use infra_mysql::{Endpoint, Mysql};

fn lab_endpoint() -> Endpoint {
    match std::env::var("FERROPRESS_MYSQL_URL") {
        Ok(url) => match Endpoint::try_from(url.as_str()) {
            Ok(endpoint) => endpoint,
            Err(_) => Endpoint::new("127.0.0.1", 3306, "reference", "reference", "reference"),
        },
        Err(_) => Endpoint::new("127.0.0.1", 3306, "reference", "reference", "reference"),
    }
}

fn live_store() -> Option<Mysql> {
    Mysql::connect(&lab_endpoint()).ok()
}

#[test]
fn refused_connection_is_unavailable() {
    let endpoint = Endpoint::new("127.0.0.1", 1, "reference", "reference", "reference");
    assert!(Mysql::connect(&endpoint).is_err());
}

#[test]
fn url_keeps_host_user_and_database_and_redacts_the_password() {
    let endpoint = Endpoint::try_from("mysql://reference:secret-pass@127.0.0.1:3306/reference");
    assert!(endpoint.is_ok());
    if let Ok(endpoint) = endpoint {
        assert_eq!(endpoint.host(), "127.0.0.1");
        assert_eq!(endpoint.port(), 3306);
        assert_eq!(endpoint.user(), "reference");
        assert_eq!(endpoint.database(), "reference");
        assert_eq!(endpoint.password(), "secret-pass");
        let shown = format!("{endpoint:?}");
        assert!(shown.contains("<redacted>"));
        assert!(!shown.contains("secret-pass"));
    }
    assert!(Endpoint::try_from("not-a-url").is_err());
}

#[test]
fn select_insert_null_and_missing_table() {
    let Some(mut store) = live_store() else {
        return;
    };
    let shown = format!("{store:?}");
    assert!(shown.contains("Mysql"));
    assert!(shown.contains("<redacted>"));

    let created = store.query(&Sql::from(
        "CREATE TEMPORARY TABLE ferropress_store_probe (
            id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
            name VARCHAR(32) NULL
        ) ENGINE=InnoDB",
    ));
    assert!(
        created
            .as_ref()
            .is_ok_and(|reply| reply.as_done().is_some())
    );

    let inserted = store.query(&Sql::from(
        "INSERT INTO ferropress_store_probe (name) VALUES ('home')",
    ));
    assert!(inserted.as_ref().is_ok_and(|reply| {
        reply
            .as_done()
            .is_some_and(|(affected, id)| affected == 1 && id > 0)
    }));

    let rows = Store::query(
        &mut store,
        &Sql::from("SELECT name FROM ferropress_store_probe ORDER BY id"),
    );
    assert!(rows.as_ref().is_ok_and(|reply| reply.as_rows().is_some()));
    if let Ok(reply) = rows
        && let Some(set) = reply.as_rows()
    {
        assert_eq!(set.columns()[0].as_bytes(), b"name");
        assert_eq!(set.rows()[0].cells(), [Cell::bytes("home")]);
    }

    let nulled = store.query(&Sql::from(
        "INSERT INTO ferropress_store_probe (name) VALUES (NULL)",
    ));
    assert!(nulled.as_ref().is_ok_and(|reply| reply.as_done().is_some()));

    let with_null = store.query(&Sql::from(
        "SELECT name FROM ferropress_store_probe WHERE name IS NULL",
    ));
    assert!(
        with_null
            .as_ref()
            .is_ok_and(|reply| reply.as_rows().is_some())
    );
    if let Ok(reply) = with_null
        && let Some(set) = reply.as_rows()
    {
        assert_eq!(set.rows()[0].cells(), [Cell::Null]);
    }

    let empty = store.query(&Sql::from(
        "SELECT name FROM ferropress_store_probe WHERE 1 = 0",
    ));
    assert!(empty.as_ref().is_ok_and(|reply| {
        reply
            .as_rows()
            .is_some_and(|set| set.columns().len() == 1 && set.rows().is_empty())
    }));

    let missing = store.query(&Sql::from("SELECT * FROM ferropress_missing_table"));
    assert!(
        missing
            .as_ref()
            .is_ok_and(|reply| { reply.as_failed().is_some_and(|error| error.code() == 1146) })
    );
}
