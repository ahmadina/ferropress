# Stamp, week and time query vars

How native crates read `m`, `w`, `hour`, `minute` and `second` after
fill-and-parse, and which archive-date flags they set. Implemented by
`crates/cms-query` (`contracts/entries/query-time.toml`). Provenance:
`wp-includes/class-wp-query.php` (`fill_query_vars`, `parse_query`) in the
pinned reference (inventory names only). SQL, hooks and PHP-visible names are
not bound yet.

Each raw value is a string of bytes, or absent. Calendar keys `year`,
`monthnum` and `day` are `contracts/query/date.md`. The caller applies these
flags only when no singular key claimed the query (`contracts/query/singular.md`).

## Fill

An absent key is filled as empty bytes. Empty bytes are then parsed as below.

## Parse

`w` is stored as `absint`: PHP `(int)` then the absolute value. Missing or
empty bytes store `0`.

`m` keeps only ASCII digits (`0`–`9`), in order. Missing bytes store empty
bytes. The stored value is that digit string, not an integer.

`hour`, `minute` and `second` stay empty when the raw bytes are missing or
empty. Any other scalar is stored as `absint`, including `0` and a
non-numeric string (which becomes `0`). Empty after parse is not `0`.

## Archive flags

Order, after the time and calendar keys have been parsed:

1. A stored `second`, `minute` or `hour` that is not empty (`0` counts as
   present) sets `is_time` and `is_date`.
2. Then the day, month and year steps in `contracts/query/date.md`. Those
   steps run only while `is_date` is still unset, so a present clock value
   skips both the calendar flags and the calendar 404.
3. A stored `m` that is PHP-string-truthy (not empty and not the single byte
   `0`) sets `is_date`. Digit length then adds one more flag: greater than 9
   is `is_time`; greater than 7 is `is_day`; greater than 5 is `is_month`;
   otherwise `is_year`. This step does not clear flags already set.
4. A stored `w` greater than zero sets `is_date` and no other date flag.

A week-only or time-only request is therefore `is_date` without `is_year`,
`is_month` or `is_day`.

## Not bound yet

Date SQL, the `wp_checkdate` filter, clock range checks, and combining these
keys with a singular query.
