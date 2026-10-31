# User insert, update and delete

How native crates write one row on the inventory `users` table. Implemented by
`crates/cms-users` (`contracts/entries/user-write.toml`). Provenance:
`wp-includes/user.php`, `wp-admin/includes/user.php` and
`wp-includes/class-wpdb.php` in the pinned reference (inventory names only).
PHP-visible functions, hooks, sanitizing, password hashing, unique logins,
nicename slugs, default dates, user meta, roles, capabilities, reassignment and
the object cache are not bound yet. Values are the raw stored bytes.

The `users` table is global: it is not prefixed with a blog id. The physical name
is still the caller-supplied prefix plus `users`.

## Columns

A write carries the compact users-table cells (no `ID`, no `user_status`):
`user_pass`, `user_nicename`, `user_email`, `user_url`, `user_registered`,
`user_activation_key`, `display_name`, and on insert `user_login`. Missing cells
are empty bytes.

Every compact cell is a MySQL string literal (`%s`), same escapes as
`contracts/options/load.md`. The row id in an update or delete `WHERE` uses the
integer format (`%d`): a decimal signed integer, unquoted.

Insert and update walk those cells in different orders, as measured below. Update
does not rewrite `user_login`.

## Insert

One statement. The table identifier and every column name are backtick-quoted.
Column order is the compact list, with `user_login` appended:

```
INSERT INTO `{table}` (`user_pass`, `user_nicename`, `user_email`, `user_url`, `user_registered`, `user_activation_key`, `display_name`, `user_login`) VALUES ({pass}, {nicename}, {email}, {url}, {registered}, {activation_key}, {display_name}, {login})
```

Measured from `wpdb::insert` on `$wpdb->users` after `wp_insert_user` compact
(7.1.3). A `Done` reply whose `insert_id` is a usable user id is inserted.
`insert_id` zero, a row set, or a server rejection is unchanged. A platform `Err`
is not an unchanged write. The new row is not remembered: the next get issues
`SELECT`.

## Update

One statement. The same cells except `user_login` are written. The row is chosen
by `ID` (`%d`):

```
UPDATE `{table}` SET `user_pass` = {pass}, `user_nicename` = {nicename}, `user_email` = {email}, `user_url` = {url}, `user_registered` = {registered}, `user_activation_key` = {activation_key}, `display_name` = {display_name} WHERE `ID` = {id}
```

Measured from `wpdb::update( $wpdb->users, $data, array( 'ID' => $user_id ) )`.
A `Done` reply with `affected` greater than zero is replaced; `affected` zero, a
row set, or a server rejection is unchanged. After the store answers, a
remembered hit for that id is dropped.

## Delete

```
DELETE FROM `{table}` WHERE `ID` = {id}
```

Measured from `wpdb::delete( $wpdb->users, array( 'ID' => $id ) )` on a single
site. `Done` / rejection / platform `Err` mean the same as update. After the
store answers, a remembered hit for that id is dropped.

## Not bound yet

`wp_insert_user` / `wp_update_user` / `wp_delete_user`, `wp_unslash`, password
hashing, unique `user_login` / `user_nicename` / `user_email`, default
`user_registered`, `user_status`, the multisite `spam` column, user meta, roles,
capabilities, post/link reassignment, `clean_user_cache`, and the PHP-visible
functions.
