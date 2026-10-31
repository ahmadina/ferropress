# Primitive capability check

How native crates decide whether a merged capability map allows a list of
primitive names. Implemented by `crates/cms-auth` (`contracts/entries/auth-has.toml`).
Provenance: `wp-includes/class-wp-user.php` (`has_cap`, after `map_meta_cap`) in
the pinned reference (inventory names only). `map_meta_cap`, the `user_has_cap`
filter, numeric user levels, super-admin short-circuit and PHP-visible names are
not bound yet.

The input map is `allcaps` from `contracts/auth/roles.md`.

## Rules

The check does not run the `user_has_cap` filter. It then applies two fixed keys
and PHP `empty` on each required name:

- `exist` is always allowed, even when the map stores it as false.
- `do_not_allow` is never allowed, even when the map stores it as true. The key
  is treated as absent.
- Every other name is allowed only when it is present and granted.
- An empty required list is allowed (PHP `array_all` of no items).
- Every required name must be allowed.

Callers pass primitive names. Mapping a capability onto this list is
`contracts/auth/map.md`.

## Not bound yet

The `user_has_cap` filter, numeric `level_N` translation, multisite super-admin,
and object-id arguments. The current-user check is `contracts/auth/current-can.md`.
