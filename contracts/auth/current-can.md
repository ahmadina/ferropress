# Current user capability check

How native crates decide whether the request's current user is allowed a
capability. Implemented by `crates/cms-auth` (`contracts/entries/auth-current-can.toml`).
Provenance: `wp-includes/capabilities.php` (`current_user_can`, `user_can`) in
the pinned reference (inventory names only). Cookies, object-id arguments, the
`user_has_cap` filter, and PHP-visible names are not bound yet.

The current user is `contracts/auth/current.md`. Mapping is `contracts/auth/map.md`.
The primitive check is `contracts/auth/has.md`.

## Unset or empty

When the current user is unset or a set missed the users table, the check uses
the anonymous actor and an empty capability map. It does not read usermeta.
`exist` is allowed. Names that map to `do_not_allow` are denied. Other names are
allowed only if that empty map grants them (they are not).

## A loaded current user

When a users-table row is current, the check is `Caps::can` for that id: load
allcaps, map the name for that stored actor, then apply the primitive check.

## Not bound yet

Object-id arguments, cookies, `determine_current_user`, super-admin, and the
`user_has_cap` filter.
