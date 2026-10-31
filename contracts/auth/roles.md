# Role table and allcaps merge

How native crates read the site role table and merge it with a user's stored
capability map. Implemented by `crates/cms-auth` (`contracts/entries/auth-roles.toml`).
Provenance: `wp-includes/class-wp-roles.php` and `wp-includes/class-wp-user.php`
(`get_role_caps`) in the pinned reference (inventory names only). `user_has_cap`,
`map_meta_cap`, the `$wp_user_roles` global, multisite blog prefixes, writes and
PHP-visible names are not bound yet.

## Role option

The table is the `{prefix}user_roles` option (single-site `get_blog_prefix` plus
`user_roles`). Reads use `contracts/options/load.md`. A first read loads the
autoload bag, then that name. Missing option, a non-array value, or bytes that
are not a serialized array are an empty table. A later read in this request does
not hit the store again. A platform `Err` from the store is not an empty table.

Each string key of the decoded array is a role name. The role's `capabilities`
field, when it is an array, uses the same grant rules as
`contracts/auth/caps.md`. A role whose value is not an array, or that has no
`capabilities` array, is still a role with an empty cap map.

## Merge

`allcaps` walks the user's stored map in written order. Every key that is a role
name contributes that role's capabilities, merged in that order (PHP
`array_merge`: first-seen keys keep their position; later values replace). The
stored map is then overlaid on top. A stored role key whose grant is false still
selects the role; the false flag wins only for that name after the overlay.

The result is `WP_User::$allcaps` before `user_has_cap`. Primitive checks that
apply `exist` and `do_not_allow` are `contracts/auth/has.md`.

## Not bound yet

The `user_has_cap` filter, `$wp_user_roles`, `add_role` / `remove_role`,
multisite `{prefix}{blog_id}_user_roles`, and the PHP-visible role objects.
