# Singular query flags

How native crates pick one singular request after the individual query vars
are parsed. Implemented by `crates/cms-query`
(`contracts/entries/query-singular.toml`). Provenance:
`wp-includes/class-wp-query.php` (`parse_query`) in the pinned reference
(inventory names only). Object matching, post-type filters, SQL, hooks and
PHP-visible names are not bound yet.

`parse_query` sets at most one of attachment, single-post, or page. Later
keys do not run when an earlier key claimed the query.

## Order

1. An attachment slug or a positive attachment id: `is_attachment` and
   `is_single` are true.
2. Else a non-empty `name`, or a positive `p`: `is_single` is true.
3. Else a non-empty `pagename`, or a positive `page_id`: `is_page` is true
   and `is_single` is false.
4. Else none of those flags are set.

`is_singular` (with an empty post-type list) is true when any of
`is_single`, `is_page`, or `is_attachment` is true.

A negative `p` has already been stored as `0` and does not claim single; a
page key may still win.

## Not bound yet

Matching a specific post, page or attachment argument, the `is_singular`
post-type list (needs the queried object), archives, home, 404 as a public
flag, SQL, and the rest of `WP_Query`.
