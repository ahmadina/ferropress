# Query var `s`

How native crates read the `s` query var after fill-and-parse, and when they
set the search flag. Implemented by `crates/cms-query`
(`contracts/entries/query-search.toml`). Provenance:
`wp-includes/class-wp-query.php` (`fill_query_vars`, `parse_query`) in the
pinned reference (inventory names only). Search SQL, `sentence`,
`search_columns`, hooks and PHP-visible names are not bound yet.

`s` is the search string. The raw value is a string of bytes, or absent.

## Fill

An absent `s` is filled as empty bytes on the parsed bag. The search flag
does not look at that fill: it looks at whether the caller supplied the key
at all.

## Parse

A supplied scalar is stored as those bytes, with no trim. A supplied value
longer than 1600 bytes is stored as empty. Missing bytes store empty.

`get_query_var( 's' )` after this parse returns those stored bytes.

## Search flag

The caller applies this flag only when no singular key claimed the query
(`contracts/query/singular.md`).

The request is a search (`is_search`) when the caller supplied `s`, even if
the stored bytes are empty (explicit empty, or a value that was longer than
1600 bytes and therefore wiped). An absent key is not a search.

## Not bound yet

Non-scalar `s` (arrays/objects), `sentence`, `search_columns`, search SQL,
and combining `s` with a singular query.
