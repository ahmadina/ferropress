# SQL store port

How native crates talk to the site database. Implemented by `crates/cms-db` (ADR-0004). The MySQL
adapter that performs the statements is `crates/infra-mysql` (`mysql` 28, text protocol). PHP-visible
names on the database object are registered later by `php-bindings`.

Provenance: `wp-includes/class-wpdb.php` in the pinned reference (inventory names only). Table stems
are the names in `contracts/inventory/tables.jsonl`.

## Statement and reply

A store executes one SQL statement, exactly the bytes it was given. It does not rewrite the text,
add a prefix, or invent a different query. The reply is one of:

- **rows**: column names in result order, then zero or more rows of cells. A cell is `NULL` or a
  byte string (the text the server sent for that field).
- **done**: how many rows the statement affected, and the last insert id the server reported (`0`
  when the statement did not create one).
- **failed**: a server-reported error (numeric code and message). This is a plugin-observable
  outcome, not a Rust `Err`.

A Rust `Err` means the adapter could not reach a server or the connection is unusable. A syntax
error, a missing table or a duplicate key is `failed`, not `Err`.

## Table names

A table **prefix** is an arbitrary byte string with no NUL (empty is allowed). A table **stem** is
an inventory name such as `options` or `posts` (not empty, no NUL). The physical name is the prefix
bytes followed by the stem bytes. The port does not know the default prefix of a site; the caller
supplies it.

## Not bound yet

Placeholders and escaping, SQL filters, transactions, multiple result sets, typed cells, and the
PHP-visible database object.
