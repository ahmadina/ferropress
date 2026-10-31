# Query var `name`

How native crates read the `name` query var after fill-and-parse. Implemented by
`crates/cms-query` (`contracts/entries/query-name.toml`). Provenance:
`wp-includes/class-wp-query.php` (`fill_query_vars`, `parse_query`) in the
pinned reference (inventory names only). Other query vars, object matching,
SQL, hooks and PHP-visible names are not bound yet.

`name` names a post slug. The raw value is a string of bytes, or absent.

## Fill

An absent `name` is filled as empty bytes. Empty bytes are then parsed as
below.

## Parse

A scalar value is stored after PHP `trim` (space, tab, newline, carriage
return, vertical tab, NUL). Missing bytes store empty. A value that is only
trim bytes stores empty.

`get_query_var( 'name' )` after this parse returns those stored bytes.

A stored value that is not empty is a single-post request when no earlier
singular key (`attachment`) claimed the query. This crate binds only the
`name` half: `is_single` is true when the stored bytes are not empty. The
bytes `0` are not empty.

## Not bound yet

Non-scalar `name` (arrays/objects), `pagename`, precedence against
`attachment`, matching a specific post argument, SQL, and the rest of
`WP_Query`.
