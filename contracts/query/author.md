# Query vars `author` and `author_name`

How native crates read `author` and `author_name` after fill-and-parse, and
when they set the author-archive flag. Implemented by `crates/cms-query`
(`contracts/entries/query-author.toml`). Provenance:
`wp-includes/class-wp-query.php` (`fill_query_vars`, `parse_query`) in the
pinned reference (inventory names only). User lookup, `author__in`,
`author__not_in`, SQL, hooks and PHP-visible names are not bound yet.

Each raw value is a string of bytes, or absent.

## Fill

An absent key is filled as empty bytes. Empty bytes are then parsed as below.

## Parse

A scalar `author` keeps only ASCII digits, comma and minus, in order.
Missing bytes store empty. The stored value is that filtered string, not an
integer.

`author_name` is stored as the supplied bytes, with no trim. Missing bytes
store empty. Path splitting and title sanitizing happen later in the posts
bag and are not bound here.

`get_query_var` after this parse returns those stored bytes.

## Author flag

The caller applies this flag only when no singular key claimed the query
(`contracts/query/singular.md`).

The request is an author archive (`is_author`) when:

1. The stored `author` string is not PHP-empty (`""` or `"0"`) and does
   not loosely equal the bytes `0` (PHP `==`). Numeric all-zero strings
   such as `00` and `-0` loosely equal `0`. A list such as `0,1` does not.
2. Or the stored `author_name` is not empty. `0` and whitespace count as
   not empty.

## Not bound yet

Non-scalar `author`, `author__in`, `author__not_in`, resolving a nicename
to a user id, author SQL, and combining these keys with a singular query.
