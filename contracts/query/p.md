# Query var `p`

How native crates read the `p` query var after fill-and-parse. Implemented by
`crates/cms-query` (`contracts/entries/query-p.toml`). Provenance:
`wp-includes/class-wp-query.php` (`fill_query_vars`, `parse_query`) in the
pinned reference (inventory names only). SQL, other query vars, hooks and
PHP-visible names are not bound yet.

`p` names a post id. The raw value is a string of bytes, or absent.

## Fill

An absent `p` is filled as empty bytes. Empty bytes are then parsed as below.

## Parse

The bytes are cast with PHP `(int)` (`contracts/runtime/operators.md`).

- When that integer is less than zero, the stored value is `0` and the request
  is a 404 (`error` = `404`).
- Otherwise the stored value is that integer.

A stored value greater than zero is a single-post request. Zero is not.

`get_query_var( 'p' )` after this parse returns the stored integer.

## Not bound yet

Non-scalar `p` (arrays/objects), every other query var, `is_404` as a public
flag beyond this 404 mark, SQL, and the rest of `WP_Query`.
