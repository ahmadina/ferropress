# Link manager capability

How native crates decide whether `manage_links` is a primitive or is denied.
Implemented by `crates/cms-auth` (`contracts/entries/auth-links.toml`).
Provenance: `wp-includes/capabilities.php` (`map_meta_cap`, `manage_links`) and
`wp-includes/option.php` (`get_option`) in the pinned reference (inventory names
only). Hooks and PHP-visible names are not bound yet.

The option name is the inventory `link_manager_enabled` row (not prefixed).
Reads use `contracts/options/load.md`. Mapping without this flag treats the
option as off.

## On or off

Missing stored bytes are off. Stored bytes are on only when they are not a PHP
empty string: empty bytes and the single byte `0` are off; any other stored
bytes are on. Serialized values are not decoded.

When the option is on, `manage_links` stays `manage_links`. When it is off, it
becomes `do_not_allow`. A later read of the flag in this request does not hit
the store again.

## Not bound yet

Writes to the option, the object cache, and unserializing a stored value.
