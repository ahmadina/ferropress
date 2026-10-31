# Post get by id

How native crates read one row from the site `posts` table. Implemented by `crates/cms-content`
(`contracts/entries/content-get.toml`). Provenance: `wp-includes/class-wp-post.php` and
`wp-includes/post.php` in the pinned reference (inventory names only). PHP-visible objects,
filters, sanitizing, writes, meta, terms and the object cache are not bound yet.

## Identifiers

A post id is an unsigned integer greater than zero. Zero and every non-numeric value are a miss
and do not hit the store.

## One row

A first read of an id issues one statement, with the physical table name (prefix + `posts`)
unquoted and the id as a decimal integer (`%d`, no quotes):

```
SELECT * FROM {table} WHERE ID = {id} LIMIT 1
```

Measured from `WP_Post::get_instance` (7.1.3). A hit is the cells the server sent, keyed by the
column names in the inventory `posts` table. SQL `NULL` becomes empty bytes. A later get of the
same id in this request does not hit the store again.

A miss is no row, a server rejection, or a row whose `ID` cell is not a usable id. A miss is not
remembered: the next get issues the statement again. A platform `Err` from the store is not a miss.

## Returned row

The value is the stored bytes, not sanitized and not filtered. Computed properties (`ancestors`,
`page_template`, `post_category`, `tags_input`) and the `filter` flag are not here.

## Not bound yet

`get_post` output formats, `sanitize_post`, the `posts` object-cache group, `wp_insert_post` /
`wp_update_post` / `wp_delete_post`, post meta, revisions, the global current post, and the
PHP-visible post object.
