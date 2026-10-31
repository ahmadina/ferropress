# Comment get by id

How native crates read one row from the site `comments` table. Implemented by
`crates/cms-comments` (`contracts/entries/comment-get.toml`). Provenance:
`wp-includes/class-wp-comment.php` in the pinned reference (inventory names only).
PHP-visible objects, filters, sanitizing, writes, comment meta, children and the
object cache are not bound yet.

## Identifiers

A comment id is an unsigned integer greater than zero. Zero and every non-numeric
value are a constructor error and do not hit the store.

## One row

A first read of an id issues one statement, with the physical table name (prefix +
`comments`) unquoted and the id as a decimal integer (`%d`, no quotes):

```
SELECT * FROM {table} WHERE comment_ID = {id} LIMIT 1
```

Measured from `WP_Comment::get_instance` (7.1.3). A hit is the cells the server sent,
keyed by the column names in the inventory `comments` table. SQL `NULL` becomes empty
bytes. A later get of the same id in this request does not hit the store again.

A miss is no row, a server rejection, or a row whose `comment_ID` cell is not a usable
id. A miss is not remembered: the next get issues the statement again. A platform `Err`
from the store is not a miss.

## Returned row

The value is the stored bytes, not sanitized and not filtered. Computed children and
the `populated_children` flag are not here.

## Not bound yet

`get_comment` output formats, `sanitize_comment`, the `comment` object-cache group,
`wp_insert_comment` / `wp_update_comment` / `wp_delete_comment`, comment meta,
children, status transitions, and the PHP-visible comment object.
