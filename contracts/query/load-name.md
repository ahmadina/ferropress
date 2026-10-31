# Name slug load

How native crates load the post row when the singular winner is a `name` slug
or an attachment slug. Implemented by `crates/cms-query`
(`contracts/entries/query-load-name.toml`). Provenance:
`wp-includes/class-wp-query.php` (`get_posts` name and attachment clauses)
and `wp_basename` in the pinned reference (inventory names only). Status/type
filters on the main SELECT, joins, pagination, hooks and PHP-visible names
are not bound yet.

The winner is `contracts/query/singular.md`. When that winner is a single-post
`name` (not a positive `p`), or an attachment slug (not a positive attachment
id), the slug is resolved through `contracts/content/name.md`. An attachment
slug is the last path segment (`wp_basename`) before sanitizing. A hit is that
row. A miss is an empty list. An id or pagename winner still uses
`contracts/query/load-id.md` or `contracts/query/load-path.md`.

## Not bound yet

The full `get_posts` SELECT assembly, combining `name` with `p`, and
attachment-by-filename.
