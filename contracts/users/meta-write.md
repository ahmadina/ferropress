# User meta insert, update and delete

How native crates write cells on the inventory `usermeta` table. Implemented by
`crates/cms-users` (`contracts/entries/user-meta-write.toml`). Provenance:
`wp-includes/meta.php` (`add_metadata`, `update_metadata`, `delete_metadata`) and
`wp-includes/class-wpdb.php` in the pinned reference (inventory names only).
PHP-visible names, hooks, sanitizing, `maybe_serialize`, unique keys, previous-value
matches, `delete_all` and the object cache are not bound yet. Values are the raw
stored bytes.

The `usermeta` table is global: it is not prefixed with a blog id. The physical
name is still the caller-supplied prefix plus `usermeta`.

## Identifiers

A user id is an unsigned integer greater than zero. A meta key is a non-empty byte
string (same rules as `contracts/users/meta.md`). A meta row id (`umeta_id`) is an
unsigned integer greater than zero. Zero and an empty key are constructor errors
and do not hit the store.

## Formats

`user_id` and `umeta_id` use the integer format (`%d`): a decimal signed integer,
unquoted. `meta_key` and `meta_value` are MySQL string literals, same escapes as
`contracts/options/load.md`.

## Insert

One statement. The table identifier and every column name are backtick-quoted.
Column order is `user_id`, `meta_key`, `meta_value`:

```
INSERT INTO `{table}` (`user_id`, `meta_key`, `meta_value`) VALUES ({id}, {key}, {value})
```

Measured from `wpdb::insert` in `add_metadata` for type `user` (7.1.3). A `Done`
reply whose `insert_id` is a usable `umeta_id` is inserted. `insert_id` zero, a
row set, or a server rejection is unchanged. A platform `Err` is not an unchanged
write. After the store answers, a remembered bag for that user is dropped.

## Update

One statement. Every matching row for the user and key is rewritten. The row is
chosen by `user_id` (`%d`) and `meta_key` (`%s`):

```
UPDATE `{table}` SET `meta_value` = {value} WHERE `user_id` = {id} AND `meta_key` = {key}
```

Measured from `wpdb::update` in `update_metadata` when the key already exists
and no previous-value clause is supplied. A `Done` reply with `affected` greater
than zero is replaced; `affected` zero, a row set, or a server rejection is
unchanged. After the store answers, a remembered bag for that user is dropped.
Selecting existing ids and falling through to insert are not bound.

## Delete

Two statements. First the row ids, with the physical table name unquoted:

```
SELECT umeta_id FROM {table} WHERE meta_key = {key} AND user_id = {id}
```

Measured from `delete_metadata` for type `user` when `delete_all` is false and
no value filter is supplied. No usable ids, a server rejection, or a `Done`
reply is unchanged and the second statement is not issued.

Otherwise the ids are deleted in the order the server returned them, unquoted
table, unquoted integers, a space after `IN(` and before `)`:

```
DELETE FROM {table} WHERE umeta_id IN( {id},{id} )
```

A `Done` reply with `affected` greater than zero is removed; `affected` zero, a
row set, or a server rejection is unchanged. After the store answers, a
remembered bag for that user is dropped. A platform `Err` on either statement
is not an unchanged write.

## Not bound yet

`add_user_meta` / `update_user_meta` / `delete_user_meta`, `wp_unslash`,
`sanitize_meta`, `maybe_serialize`, unique-key short-circuit, previous-value
match, same-value short-circuit, `delete_all`, a value filter on delete, other
meta types, and the `{type}_meta` object-cache group. Reads are
`contracts/users/meta.md`.
