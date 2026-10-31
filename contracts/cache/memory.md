# Request-local object cache

How native crates store values by key and group for one request. Implemented by
`crates/cms-cache` (`contracts/entries/cache-memory.toml`). Provenance:
`wp-includes/class-wp-object-cache.php` and `wp-includes/cache.php` in the pinned reference
(inventory names only). PHP-visible names, object cloning, numeric incr/decr, multi-key helpers,
`wp_suspend_cache_addition`, persistent backends and the options bag are not bound yet. Values are
opaque bytes.

## Keys

A key is either an integer (including `0`) or a byte string that is not empty after PHP `trim`
(`space`, tab, newline, carriage return, vertical tab, NUL). The stored key is the bytes as given;
they are not trimmed. A string that is only those trim bytes is rejected. A float is not a key.

## Groups

On add, set, replace, get and delete, an empty group name or the single byte `0` becomes `default`
(PHP `empty`). Other names, including a space, stay as given. `flush_group` uses the name as given
and does not apply that rewrite, so flushing `''` does not clear `default`.

## Site prefix

On a single site, keys are stored as given. On a network of sites, a non-global group stores the
key as `{blog_id}:` plus the key bytes (`1` → `1:home`). Global groups skip that prefix. Switching
the current site changes the prefix for later lookups; existing rows stay under the prefix they
were written with. On a single site, switching the current site does not add a prefix.

## Add, set, replace, get, delete

`add` writes only when the resolved key is missing. `set` always writes. `replace` writes only when
the resolved key is present. `delete` removes a present key. A hit returns the stored bytes, including
empty; a miss is `None`. `get` increments a hit or miss counter. Those counters do not move for an
invalid key, because a native key is rejected at construction.

The `expire` argument of the reference is unused in this memory bag: a request ends, and the bag
goes with it. The `force` argument of get is unused (there is no persistent backend).

## Flush

`flush` drops every group. `flush_group` drops one group name. Both always succeed, including when
the group was already empty.

## Not bound yet

`add_multiple` / `get_multiple` / `set_multiple` / `delete_multiple`, `incr` / `decr`, salted
helpers, last-changed stamps, `wp_cache_supports`, `wp_cache_close`, non-persistent groups,
`wp_suspend_cache_addition`, object clone on set/get, PHP `false`/`null` as stored values, the
PHP-visible functions, and wiring this bag into `cms-options`.
