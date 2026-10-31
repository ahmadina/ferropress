# Query var `page_id`

How native crates read the `page_id` query var after fill-and-parse. Implemented
by `crates/cms-query` (`contracts/entries/query-page-id.toml`). Provenance:
`wp-includes/class-wp-query.php` (`fill_query_vars`, `parse_query`) and
`wp-includes/load.php` (`absint`) in the pinned reference (inventory names
only). Other query vars, object matching, SQL, hooks and PHP-visible names are
not bound yet.

`page_id` names a page id. The raw value is a string of bytes, or absent.

## Fill

An absent `page_id` is filled as empty bytes. Empty bytes are then parsed as
below.

## Parse

A scalar value is stored as `absint`: PHP `(int)` (`contracts/runtime/operators.md`)
then the absolute value. Missing or empty bytes store `0`. A negative integer
becomes its positive counterpart and is not a 404.

`get_query_var( 'page_id' )` after this parse returns that stored integer.

A stored value greater than zero is a page request when no earlier singular key
(`attachment`, `name`, `p`) claimed the query. This crate binds only the
`page_id` half: `is_page` is true when the stored value is greater than zero.

## Not bound yet

Non-scalar `page_id` (arrays/objects), `pagename`, precedence against
`attachment` / `name` / `p`, matching a specific page argument, SQL, and the
rest of `WP_Query`.
