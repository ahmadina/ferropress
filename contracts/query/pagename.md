# Query var `pagename`

How native crates read the `pagename` query var after fill-and-parse.
Implemented by `crates/cms-query` (`contracts/entries/query-pagename.toml`).
Provenance: `wp-includes/class-wp-query.php` (`fill_query_vars`, `parse_query`)
in the pinned reference (inventory names only). Other query vars, object
matching, SQL, hooks and PHP-visible names are not bound yet.

`pagename` names a page slug. The raw value is a string of bytes, or absent.

## Fill

An absent `pagename` is filled as empty bytes. Empty bytes are then parsed as
below.

## Parse

A scalar value is stored after PHP `trim` (space, tab, newline, carriage
return, vertical tab, NUL). Missing bytes store empty. A value that is only
trim bytes stores empty.

`get_query_var( 'pagename' )` after this parse returns those stored bytes.

A stored value that is not empty is a page request when no earlier singular
key (`attachment`, `name`, `p`) claimed the query. This crate binds only the
`pagename` half: `is_page` is true when the stored bytes are not empty. The
bytes `0` are not empty.

## Not bound yet

Non-scalar `pagename` (arrays/objects), precedence against `attachment` /
`name` / `p`, matching a specific page argument, SQL, and the rest of
`WP_Query`.
