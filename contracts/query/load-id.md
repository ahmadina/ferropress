# Singular id load

How native crates load the post row when a parsed singular query names an id.
Implemented by `crates/cms-query` (`contracts/entries/query-load-id.toml`).
Provenance: `wp-includes/class-wp-query.php` (`get_posts` id clause) and
`WP_Post::get_instance` in the pinned reference (inventory names only). Slug
lookups, status/type filters, joins, pagination, hooks and PHP-visible names
are not bound yet.

The winner is `contracts/query/singular.md`. When that winner is an attachment
id, a positive `p`, or a positive `page_id`, one row is read through the posts
bag (`contracts/content/get.md`):

```
SELECT * FROM {table} WHERE ID = {id} LIMIT 1
```

A hit is that row. A miss is an empty list. A name-only or slug-only winner
issues no SQL.

## Not bound yet

`get_posts` / `WP_Query` SELECT assembly (`SQL_CALC_FOUND_ROWS`, post type and
status, `ORDER BY`, `LIMIT`), `name` / `pagename` path lookups, filters, and
the queried object.
