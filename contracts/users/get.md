# User get by id

How native crates read one row from the inventory `users` table. Implemented by
`crates/cms-users` (`contracts/entries/user-get.toml`). Provenance:
`wp-includes/class-wp-user.php` in the pinned reference (inventory names only).
PHP-visible objects, caps, meta, lookups by login / email / nicename, writes,
sanitizing and the object cache are not bound yet.

The `users` table is global: it is not prefixed with a blog id. The physical name
is still the caller-supplied prefix plus `users`.

## Identifiers

A user id is an unsigned integer greater than zero. Zero and every non-numeric
value are a constructor error and do not hit the store.

## One row

A first read of an id issues one statement, with the physical table name (prefix +
`users`) unquoted. The id is a quoted MySQL string (`%s`), even though the cell is
numeric:

```
SELECT * FROM {table} WHERE ID = '{id}' LIMIT 1
```

Measured from `WP_User::get_data_by` with field `id` (7.1.3). A hit is the cells
the server sent, keyed by the column names in the inventory `users` table. SQL
`NULL` becomes empty bytes. A later get of the same id in this request does not
hit the store again.

A miss is no row, a server rejection, or a row whose `ID` cell is not a usable id.
A miss is not remembered: the next get issues the statement again. A platform
`Err` from the store is not a miss.

## Returned row

The value is the stored bytes, not sanitized and not filtered. Computed caps,
roles and the `filter` flag are not here.

## Not bound yet

`get_user_by` fields other than id, `sanitize_user`, the `users` object-cache
group, user meta, capabilities, `wp_insert_user` / `wp_update_user` /
`wp_delete_user`, and the PHP-visible user object.
