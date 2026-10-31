# Post get by name

How native crates read one posts-table row by `post_name`. Implemented by
`crates/cms-content` (`contracts/entries/query-load-name.toml`). Provenance:
the `post_name` clause in `wp-includes/class-wp-query.php` (`get_posts`) and
`WP_Post::get_instance` row shape in the pinned reference (inventory names
only). Type/status filters, joins, pagination, hooks and PHP-visible names
are not bound yet.

The raw bytes are passed through the query-title dash sanitizer
(`sanitize_title_for_query`). One statement is issued, with the physical table
name unquoted and the slug as a prepared string literal:

```
SELECT * FROM {table} WHERE post_name = {quoted} LIMIT 1
```

A hit is the first row, remembered for this request like
`contracts/content/get.md`. A miss is no row or a server rejection, and is not
remembered. This is not the `get_posts` SELECT assembly.

## Not bound yet

`SQL_CALC_FOUND_ROWS`, post type and status, `ORDER BY`, `LIMIT` paging, and
`{$wpdb->posts}.post_name` qualification.
