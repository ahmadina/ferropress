# Current user

How native crates remember which user is current for one request. Implemented by
`crates/cms-auth` (`contracts/entries/auth-current.toml`). Provenance:
`wp-includes/pluggable.php` and `wp-includes/user.php` in the pinned reference
(inventory names only). Cookies, `determine_current_user`, login lookup by name,
`setup_userdata` and the `set_current_user` action are not bound yet. Capability
checks are `contracts/auth/current-can.md`.

## Start of the request

The current user starts unset. A read of an unset current user is empty and does
not hit the store. Cookie authentication and the `determine_current_user` filter
are not here.

## Set by id

A user id greater than zero becomes current only after a users-table get
(`contracts/users/get.md`). A hit is that stored row. A miss, or a users-table
`Done` / `Failed`, leaves the current user empty (the reference constructs a
user whose id is zero). A platform `Err` from the store is not an empty current
user.

Setting the same id that is already current does not hit the store again.

## Clear

Clearing the current user makes the next read empty. The reference does this by
setting id zero; that path does not select a row. Clearing when the current user
is already empty does not hit the store.

## Not bound yet

`wp_validate_auth_cookie`, `determine_current_user`, XML-RPC forcing id zero,
`wp_set_current_user` by login name, `setup_userdata`, the `set_current_user`
action, password checks, and the PHP-visible current-user object. Stored
capability maps are `contracts/auth/caps.md`.
