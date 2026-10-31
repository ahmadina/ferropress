# User meta get by user and key

How native crates read cells from the inventory `usermeta` table. Implemented by
`crates/cms-users` (`contracts/entries/user-meta.toml`). Provenance:
`wp-includes/meta.php` (`update_meta_cache`, `get_metadata_raw`) and
`wp-includes/user.php` (`get_user_meta`) in the pinned reference (inventory names
only). PHP-visible names, filters, `maybe_unserialize`, writes, other meta types
and the object cache are not bound yet. Values are the raw stored bytes.

The `usermeta` table is global: it is not prefixed with a blog id. The physical
name is still the caller-supplied prefix plus `usermeta`.

## Identifiers

A user id is an unsigned integer greater than zero. Zero is a constructor error
and does not hit the store.

A meta key is a non-empty byte string. The empty key (the reference's "return
every key" form) is a constructor error and is not bound. Keys are not trimmed
and are case-sensitive: `Nickname` and `nickname` are different keys.

## One user

A first read of a user id issues one statement, with the physical table name
(prefix + `usermeta`) unquoted. The id is a decimal integer in an `IN` list
(`intval`, no quotes). Rows are ordered by `umeta_id` ascending:

```
SELECT user_id, meta_key, meta_value FROM {table} WHERE user_id IN ({id}) ORDER BY umeta_id ASC
```

Measured from `update_meta_cache` for type `user` (7.1.3). Each row is stored
under its `meta_key`. The same key may appear more than once; values keep the
`umeta_id` order. SQL `NULL` becomes empty bytes. A later get of any key for
that user in this request does not hit the store again.

No rows, a server rejection, or a `Done` reply is remembered as an empty bag
for that user, so the store is not asked again. Rows whose `user_id` is not the
requested id, or whose `meta_key` is empty, are skipped. A platform `Err` from
the store is not a miss and is not remembered.

## Returned cells

`get` for a key is every stored value for that key, or none when the key is
absent. `first` is the first of those values, or none. The bytes are not
unserialized and not filtered.

## Not bound yet

`get_user_meta` with an empty key, `maybe_unserialize`, the
`get_user_metadata` filter, `update_user_metadata_cache`, writes
(`add`/`update`/`delete`), other meta types (`post`, `comment`, `term`, `blog`),
and the `{type}_meta` object-cache group.
