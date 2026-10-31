# Option load and get

How native crates read the site `options` table. Implemented by `crates/cms-options`
(`contracts/entries/option-load.toml`). Provenance: `wp-includes/option.php` in the pinned
reference (inventory names only). PHP-visible names, hooks, writes, serialization and the
object cache are not bound yet.

## Names

An option name is trimmed of leading and trailing space, tab, newline, carriage return, vertical
tab and NUL. What remains must not be empty and must not be the single byte `0` (the reference
treats that as empty). Two old names are rewritten: `blacklist_keys` becomes `disallowed_keys`,
and `comment_whitelist` becomes `comment_previously_approved`.

## Autoload read

The first read of the bag issues one statement, with the physical table name (prefix + `options`)
unquoted:

```
SELECT option_name, option_value FROM {table} WHERE autoload IN ( 'yes', 'on', 'auto-on', 'auto' )
```

If that reply is no rows, or the server rejected the statement, a second statement loads every
row:

```
SELECT option_name, option_value FROM {table}
```

Each row is stored under its name as the value bytes the server sent (`NULL` becomes empty).
Later gets of those names do not hit the store again.

## One name

A name that is not in the autoload bag is looked up once:

```
SELECT option_value FROM {table} WHERE option_name = {quoted} LIMIT 1
```

`{quoted}` is a MySQL string literal: wrapped in `'`, with `NUL` as `\0`, newline as `\n`,
carriage return as `\r`, `0x1A` as `\Z`, and `\`, `'` and `"` backslash-escaped. Other bytes,
including `%`, stay as themselves. Measured from `$wpdb->prepare` + the query the server
actually ran (7.1.3 lab).

A hit is cached for that name. A miss (no row, or a server rejection) is remembered so the
store is not asked again. A platform `Err` from the store is not a miss.

## Returned bytes

The value is the stored bytes, not unserialized. If the name is `home` and those bytes are
empty, the value of `siteurl` is returned instead. For `siteurl`, `home`, `category_base` and
`tag_base`, trailing `/` and `\` bytes are stripped.

## Not bound yet

`add` / `update` / `delete`, autoload writes, `pre_option` and other hooks, `maybe_unserialize`,
the persistent object cache, network/site options, transients, and the PHP-visible functions.
