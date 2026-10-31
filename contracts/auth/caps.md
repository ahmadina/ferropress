# Stored capability map

How native crates read the capability map stored on one user. Implemented by
`crates/cms-auth` (`contracts/entries/auth-caps.toml`). Provenance:
`wp-includes/class-wp-user.php` (`get_caps_data`, `for_site`) in the pinned
reference (inventory names only). Role merge, `user_has_cap`, `map_meta_cap`,
multisite blog prefixes, writes and PHP-visible names are not bound yet.

The map lives in the inventory `usermeta` table under the key that is the caller
table prefix followed by `capabilities` (single-site `get_blog_prefix`). Reads
use `contracts/users/meta.md`.

## One user

A first read of a user id loads that user's usermeta bag and takes the first
value of `{prefix}capabilities`. Missing meta, a non-array value, or bytes that
are not a serialized array are an empty map. A later read of the same id in this
request does not hit the store again. A platform `Err` from the store is not an
empty map.

## Serialized array

The stored bytes are PHP's serialize form (`contracts/runtime/serialized-format.md`).
Each string key is a capability or role name. The grant flag is PHP's `empty`
inverted: `true`, a non-zero number, and a non-`0` non-empty string are granted;
`false`, `0`, `0.0`, `''`, `'0'`, `null`, and an empty array are denied. Integer
keys, empty string keys, and reference tokens are skipped. Later entries for the
same name replace earlier ones (PHP array order).

The returned map is the stored `caps` property, not `allcaps`. Role capabilities
from the `user_roles` option are not merged.

## Not bound yet

`WP_User::get_role_caps`, `wp_roles`, `user_can`, `current_user_can`,
`map_meta_cap`, the `user_has_cap` filter, `exist` / `do_not_allow`, multisite
`{prefix}{blog_id}_capabilities`, writes (`add_role`, `add_cap`), and the
PHP-visible user object.
